use crate::gir::Graph;
use crate::gir_optimize::{
    optimize_gir, GirOptimizationIssue, GirOptimizationReport,
};
use crate::gir_validate::ValidationIssue;
use crate::invariants::{
    ledger_for_validated_graph, validate_preservation_with_retired_nodes,
    InvariantLedger, PreservationIssue,
};
use crate::machine::{
    linear_scan_allocate, register_pressure, AllocationIssue,
    AllocationResult, MachineProfile, RegisterPressure,
};
use crate::mir::{lower_graph, LoweringIssue, MirProgram};
use crate::machine_ir::{
    lower_mir as lower_machine_ir, MachineLoweringIssue, MachineProgram,
};
use crate::mir_validate::{validate_mir, MirIssue};
use crate::source_map::{build_source_map, SourceMap};
use crate::side_channel::{validate_side_channels, SideChannelIssue};
use crate::x86_codegen::{emit_x86_64, X86CodegenIssue};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledGraph {
    pub graph_name: String,
    pub mir: MirProgram,
    pub allocation: AllocationResult,
    pub pressure: RegisterPressure,
    pub machine_ir: MachineProgram,
    pub assembly: String,
    pub invariants: InvariantLedger,
    pub source_map: SourceMap,
    pub optimization: GirOptimizationReport,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineIssue {
    Gir(Vec<ValidationIssue>),
    SideChannel(Vec<SideChannelIssue>),
    Optimize(Vec<GirOptimizationIssue>),
    InvariantPreservation(Vec<PreservationIssue>),
    Lowering(Vec<LoweringIssue>),
    Mir(Vec<MirIssue>),
    Allocation(AllocationIssue),
    Machine(Vec<MachineLoweringIssue>),
    Codegen(Vec<X86CodegenIssue>),
}

pub fn compile_graph(
    graph: &Graph,
    machine: MachineProfile,
) -> Result<CompiledGraph, PipelineIssue> {
    if let Err(report) = crate::gir_validate::validate(graph) {
        return Err(PipelineIssue::Gir(report.issues));
    }
    if let Err(issues) = validate_side_channels(graph) {
        return Err(PipelineIssue::SideChannel(issues));
    }

    let baseline_invariants = ledger_for_validated_graph(graph);
    let (optimized_graph, optimization) =
        optimize_gir(graph).map_err(PipelineIssue::Optimize)?;
    let invariants = ledger_for_validated_graph(&optimized_graph);
    validate_preservation_with_retired_nodes(
        &baseline_invariants,
        &invariants,
        &optimization.removed_nodes,
    )
    .map_err(PipelineIssue::InvariantPreservation)?;

    let mir =
        lower_graph(&optimized_graph).map_err(PipelineIssue::Lowering)?;
    validate_mir(&mir).map_err(PipelineIssue::Mir)?;
    let allocation =
        linear_scan_allocate(&mir, machine).map_err(PipelineIssue::Allocation)?;
    let pressure = register_pressure(&allocation);
    let machine_ir =
        lower_machine_ir(&mir, &allocation).map_err(PipelineIssue::Machine)?;
    let source_map = build_source_map(&machine_ir);
    let assembly =
        emit_x86_64(&machine_ir).map_err(PipelineIssue::Codegen)?;

    Ok(CompiledGraph {
        graph_name: graph.name.clone(),
        mir,
        allocation,
        pressure,
        machine_ir,
        assembly,
        invariants,
        source_map,
        optimization,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhysicalCostSignal {
    SpillCount,
    StackBytes,
    PeakLiveValues,
}

impl CompiledGraph {
    pub fn cost_signal(&self, signal: PhysicalCostSignal) -> u128 {
        match signal {
            PhysicalCostSignal::SpillCount => self.pressure.spills as u128,
            PhysicalCostSignal::StackBytes => self.pressure.stack_bytes as u128,
            PhysicalCostSignal::PeakLiveValues => {
                self.pressure.peak_live_values as u128
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{
        AuthorityMode, Edge, IntegerType, Literal, Node, Operation, Port,
        SemanticType, SourceEndpoint, TargetEndpoint,
    };

    fn int(min: i128, max: i128) -> SemanticType {
        SemanticType::Integer(IntegerType::new(min, max).unwrap())
    }

    fn graph() -> Graph {
        Graph {
            name: "answer".into(),
            inputs: vec![],
            outputs: vec![Port {
                id: 0,
                name: "answer".into(),
                ty: int(42, 42),
            }],
            nodes: vec![
                Node {
                    id: 1,
                    operation: Operation::Const(Literal::Integer(20)),
                    inputs: vec![],
                    outputs: vec![Port {
                        id: 0,
                        name: "left".into(),
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
                        name: "right".into(),
                        ty: int(22, 22),
                    }],
                    effects: BTreeSet::new(),
                    required_capabilities: BTreeSet::new(),
                },
                Node {
                    id: 3,
                    operation: Operation::Add,
                    inputs: vec![
                        Port {
                            id: 0,
                            name: "left".into(),
                            ty: int(20, 20),
                        },
                        Port {
                            id: 1,
                            name: "right".into(),
                            ty: int(22, 22),
                        },
                    ],
                    outputs: vec![Port {
                        id: 0,
                        name: "answer".into(),
                        ty: int(42, 42),
                    }],
                    effects: BTreeSet::new(),
                    required_capabilities: BTreeSet::new(),
                },
            ],
            edges: vec![
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
            ],
            authority: AuthorityMode::DefaultDeny,
        }
    }

    fn spill_graph() -> Graph {
        let inputs: Vec<Port> = (0..6)
            .map(|id| Port {
                id,
                name: format!("x{id}"),
                ty: int(0, 10),
            })
            .collect();

        let mut nodes = Vec::new();
        for step in 0..5_u32 {
            let max = 20 + step as i128 * 10;
            nodes.push(Node {
                id: 10 + step,
                operation: Operation::Add,
                inputs: vec![
                    Port {
                        id: 0,
                        name: "left".into(),
                        ty: if step == 0 {
                            int(0, 10)
                        } else {
                            int(0, max - 10)
                        },
                    },
                    Port {
                        id: 1,
                        name: "right".into(),
                        ty: int(0, 10),
                    },
                ],
                outputs: vec![Port {
                    id: 0,
                    name: "sum".into(),
                    ty: int(0, max),
                }],
                effects: BTreeSet::new(),
                required_capabilities: BTreeSet::new(),
            });
        }

        let mut edges = vec![
            Edge {
                from: SourceEndpoint::GraphInput(0),
                to: TargetEndpoint::NodeInput { node: 10, port: 0 },
            },
            Edge {
                from: SourceEndpoint::GraphInput(1),
                to: TargetEndpoint::NodeInput { node: 10, port: 1 },
            },
        ];

        for step in 1..5_u32 {
            edges.push(Edge {
                from: SourceEndpoint::NodeOutput {
                    node: 9 + step,
                    port: 0,
                },
                to: TargetEndpoint::NodeInput {
                    node: 10 + step,
                    port: 0,
                },
            });
            edges.push(Edge {
                from: SourceEndpoint::GraphInput((step + 1) as u16),
                to: TargetEndpoint::NodeInput {
                    node: 10 + step,
                    port: 1,
                },
            });
        }
        edges.push(Edge {
            from: SourceEndpoint::NodeOutput { node: 14, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        });

        Graph {
            name: "spill".into(),
            inputs,
            outputs: vec![Port {
                id: 0,
                name: "sum".into(),
                ty: int(0, 60),
            }],
            nodes,
            edges,
            authority: AuthorityMode::DefaultDeny,
        }
    }

    #[test]
    fn compiler_pipeline_supports_typed_integer_spills() {
        let compiled =
            compile_graph(&spill_graph(), MachineProfile::x86_64_v3())
                .unwrap();

        assert!(compiled.pressure.spills >= 1);
        assert!(compiled.pressure.stack_bytes > 0);
        assert!(compiled.assembly.contains("[rbp-"));
    }

    #[test]
    fn compiler_pipeline_reaches_machine_allocation() {
        let compiled =
            compile_graph(&graph(), MachineProfile::x86_64_v3()).unwrap();

        assert_eq!(compiled.graph_name, "answer");
        assert_eq!(compiled.mir.instructions.len(), 1);
        assert!(compiled.optimization.folded_nodes.contains(&3));
        assert!(compiled.optimization.removed_nodes.contains(&1));
        assert!(compiled.optimization.removed_nodes.contains(&2));
        assert!(!compiled.allocation.locations.is_empty());
        assert!(!compiled.machine_ir.operations.is_empty());
        assert!(compiled.assembly.contains("g0_machine_main"));
        assert!(compiled.invariants.compilation_allowed());
    }
}
