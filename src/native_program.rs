use std::collections::BTreeMap;

use crate::abi::graph_symbol;
use crate::call_graph::{build_call_graph, reachable_from, CallGraphIssue};
use crate::compiler::{compile_graph, CompiledGraph, PipelineIssue};
use crate::machine::MachineProfile;
use crate::program::{validate_program, PlatformContract, ProgramContract, ProgramIssue};
use crate::x86_codegen::{
    emit_entry_wrapper, emit_x86_64_named, X86CodegenIssue,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledProgram {
    pub entry_graph: String,
    pub assembly: String,
    pub graphs: BTreeMap<String, CompiledGraph>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramCompileIssue {
    Program(Vec<ProgramIssue>),
    MissingEntry,
    CallGraph(Vec<CallGraphIssue>),
    ExternalSubgraphsNotLinked,
    Graph {
        graph: String,
        issue: PipelineIssue,
    },
    Codegen {
        graph: String,
        issues: Vec<X86CodegenIssue>,
    },
    EntryWrapper(X86CodegenIssue),
}

pub fn compile_program(
    program: &ProgramContract,
    platform: &PlatformContract,
    machine: MachineProfile,
) -> Result<CompiledProgram, ProgramCompileIssue> {
    validate_program(program, platform)
        .map_err(ProgramCompileIssue::Program)?;

    let entry = program
        .entry_graph
        .as_ref()
        .ok_or(ProgramCompileIssue::MissingEntry)?
        .clone();

    if !program.external_subgraphs.is_empty() {
        return Err(ProgramCompileIssue::ExternalSubgraphsNotLinked);
    }

    let call_graph = build_call_graph(
        &program.graphs,
        &program.external_subgraphs,
    )
    .map_err(ProgramCompileIssue::CallGraph)?;
    let reachable = reachable_from(&call_graph, &entry);

    let mut graphs = BTreeMap::new();
    let mut assembly = String::new();

    for graph_name in &reachable {
        let graph = program
            .graphs
            .iter()
            .find(|graph| graph.name == *graph_name)
            .expect("validated reachable graph");

        let mut compiled = compile_graph(graph, machine).map_err(|issue| {
            ProgramCompileIssue::Graph {
                graph: graph.name.clone(),
                issue,
            }
        })?;

        let symbol = graph_symbol(&graph.name);
        compiled.assembly = emit_x86_64_named(
            &compiled.machine_ir,
            &symbol,
            false,
        )
        .map_err(|issues| ProgramCompileIssue::Codegen {
            graph: graph.name.clone(),
            issues,
        })?;

        assembly.push_str(&compiled.assembly);
        assembly.push('
');
        graphs.insert(graph.name.clone(), compiled);
    }

    let entry_symbol = graph_symbol(&entry);
    assembly.push_str(
        &emit_entry_wrapper(&entry_symbol)
            .map_err(ProgramCompileIssue::EntryWrapper)?,
    );
    assembly.push_str(".section .note.GNU-stack,\"\" ,@progbits\n");

    Ok(CompiledProgram {
        entry_graph: entry,
        assembly,
        graphs,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{
        Edge, Graph, IntegerType, Literal, Node, Operation, Port,
        SemanticType, SourceEndpoint, TargetEndpoint,
    };

    fn int(min: i128, max: i128) -> SemanticType {
        SemanticType::Integer(IntegerType::new(min, max).unwrap())
    }

    fn worker() -> Graph {
        let mut graph = Graph::new("worker");
        graph.inputs = vec![
            Port {
                id: 0,
                name: "a".into(),
                ty: int(0, 100),
            },
            Port {
                id: 1,
                name: "b".into(),
                ty: int(0, 100),
            },
        ];
        graph.outputs = vec![Port {
            id: 0,
            name: "sum".into(),
            ty: int(0, 200),
        }];
        graph.nodes.push(Node {
            id: 1,
            operation: Operation::Add,
            inputs: graph.inputs.clone(),
            outputs: graph.outputs.clone(),
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        });
        graph.edges = vec![
            Edge {
                from: SourceEndpoint::GraphInput(0),
                to: TargetEndpoint::NodeInput { node: 1, port: 0 },
            },
            Edge {
                from: SourceEndpoint::GraphInput(1),
                to: TargetEndpoint::NodeInput { node: 1, port: 1 },
            },
            Edge {
                from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
                to: TargetEndpoint::GraphOutput(0),
            },
        ];
        graph
    }

    fn main_graph() -> Graph {
        let mut graph = Graph::new("main");
        graph.outputs = vec![Port {
            id: 0,
            name: "answer".into(),
            ty: int(0, 200),
        }];
        graph.nodes = vec![
            Node {
                id: 1,
                operation: Operation::Const(Literal::Integer(20)),
                inputs: vec![],
                outputs: vec![Port {
                    id: 0,
                    name: "v".into(),
                    ty: int(20, 20),
                }],
                effects: BTreeSet::new(),
                required_capabilities: BTreeSet::new(),
            },
            Node {
                id: 2,
                operation: Operation::Const(Literal::Integer(22)),
                inputs: vec![],
                outputs: vec![Port {
                    id: 0,
                    name: "v".into(),
                    ty: int(22, 22),
                }],
                effects: BTreeSet::new(),
                required_capabilities: BTreeSet::new(),
            },
            Node {
                id: 3,
                operation: Operation::Subgraph("worker".into()),
                inputs: vec![
                    Port {
                        id: 0,
                        name: "a".into(),
                        ty: int(0, 100),
                    },
                    Port {
                        id: 1,
                        name: "b".into(),
                        ty: int(0, 100),
                    },
                ],
                outputs: vec![Port {
                    id: 0,
                    name: "sum".into(),
                    ty: int(0, 200),
                }],
                effects: BTreeSet::new(),
                required_capabilities: BTreeSet::new(),
            },
        ];
        graph.edges = vec![
            Edge {
                from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
                to: TargetEndpoint::NodeInput { node: 3, port: 0 },
            },
            Edge {
                from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
                to: TargetEndpoint::NodeInput { node: 3, port: 1 },
            },
            Edge {
                from: SourceEndpoint::NodeOutput { node: 3, port: 0 },
                to: TargetEndpoint::GraphOutput(0),
            },
        ];
        graph
    }

    #[test]
    fn only_reachable_graphs_are_emitted() {
        let mut program = ProgramContract::default();
        program.entry_graph = Some("main".into());
        program.graphs = vec![
            main_graph(),
            worker(),
            Graph::new("dead_library_graph"),
        ];

        let platform = crate::program::test_platform();
        let compiled = compile_program(
            &program,
            &platform,
            MachineProfile::x86_64_v3(),
        )
        .unwrap();

        assert!(compiled.graphs.contains_key("main"));
        assert!(compiled.graphs.contains_key("worker"));
        assert!(!compiled.graphs.contains_key("dead_library_graph"));
        assert!(compiled.assembly.contains("g0_machine_main"));
        assert!(compiled.assembly.contains(&graph_symbol("worker")));
    }
}
