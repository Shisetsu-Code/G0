use crate::gir::Graph;
use crate::gir_validate::ValidationIssue;
use crate::invariants::{ledger_for_validated_graph, InvariantLedger};
use crate::machine::{
    linear_scan_allocate, register_pressure, AllocationIssue,
    AllocationResult, MachineProfile, RegisterPressure,
};
use crate::mir::{lower_graph, LoweringIssue, MirProgram};
use crate::machine_ir::{
    lower_mir as lower_machine_ir, MachineLoweringIssue, MachineProgram,
};
use crate::mir_validate::{validate_mir, MirIssue};
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineIssue {
    Gir(Vec<ValidationIssue>),
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

    let invariants = ledger_for_validated_graph(graph);
    let mir = lower_graph(graph).map_err(PipelineIssue::Lowering)?;
    validate_mir(&mir).map_err(PipelineIssue::Mir)?;
    let allocation =
        linear_scan_allocate(&mir, machine).map_err(PipelineIssue::Allocation)?;
    let pressure = register_pressure(&allocation);
    let machine_ir =
        lower_machine_ir(&mir, &allocation).map_err(PipelineIssue::Machine)?;
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
            outputs: vec![],
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
            ],
            authority: AuthorityMode::DefaultDeny,
        }
    }

    #[test]
    fn compiler_pipeline_reaches_machine_allocation() {
        let compiled =
            compile_graph(&graph(), MachineProfile::x86_64_v3()).unwrap();

        assert_eq!(compiled.graph_name, "answer");
        assert_eq!(compiled.mir.instructions.len(), 3);
        assert!(!compiled.allocation.locations.is_empty());
        assert!(!compiled.machine_ir.operations.is_empty());
        assert!(compiled.assembly.contains("g0_machine_main"));
        assert!(compiled.invariants.compilation_allowed());
    }
}
