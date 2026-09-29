use std::collections::{BTreeMap, BTreeSet};

use crate::gir::{
    Graph, IntegerType, Literal, NodeId, Operation, SemanticType, SourceEndpoint,
    TargetEndpoint,
};
use crate::memory::{choose_integer_width, IntegerWidth, MemoryDomain};

pub type ValueId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirType {
    Bool,
    Integer(IntegerWidth),
    Float32,
    Float64,
    Pointer,
    Bytes,
    TextHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareKind {
    Eq,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoolBinaryKind {
    And,
    Or,
    Xor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArithmeticMode {
    Checked,
    Saturating,
    Wrapping,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirOp {
    ConstInteger(i128),
    ConstBool(bool),
    Add {
        mode: ArithmeticMode,
    },
    Sub {
        mode: ArithmeticMode,
    },
    Mul {
        mode: ArithmeticMode,
    },
    Compare {
        kind: CompareKind,
        width: IntegerWidth,
        signed: bool,
    },
    BoolBinary {
        kind: BoolBinaryKind,
    },
    BoolNot,
    Load,
    Store,
    Move,
    Copy,
    Call {
        target: String,
    },
    SelectCall {
        when_true: String,
        when_false: String,
    },
    LoopCall {
        condition: String,
        body: String,
        max_iterations: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirValue {
    pub id: ValueId,
    pub ty: MirType,
    pub location: Option<MemoryDomain>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirInstruction {
    pub source_node: NodeId,
    pub op: MirOp,
    pub inputs: Vec<ValueId>,
    pub outputs: Vec<ValueId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MirProgram {
    pub values: BTreeMap<ValueId, MirValue>,
    pub inputs: Vec<ValueId>,
    pub instructions: Vec<MirInstruction>,
    pub outputs: Vec<ValueId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoweringIssue {
    InvalidGraph,
    UnsupportedOperation {
        node: NodeId,
        operation: String,
    },
    UnsupportedType {
        node: NodeId,
    },
    MissingInputValue {
        node: NodeId,
        port: u16,
    },
    RawCycle,
}

pub fn lower_graph(graph: &Graph) -> Result<MirProgram, Vec<LoweringIssue>> {
    if crate::gir_validate::validate(graph).is_err() {
        return Err(vec![LoweringIssue::InvalidGraph]);
    }

    let order = topological_order(graph)
        .ok_or_else(|| vec![LoweringIssue::RawCycle])?;

    let mut program = MirProgram::default();
    let mut next_value: ValueId = 0;
    let mut output_values = BTreeMap::<(NodeId, u16), ValueId>::new();
    let mut graph_inputs = BTreeMap::<u16, ValueId>::new();

    for input in &graph.inputs {
        let Some(ty) = lower_type(&input.ty) else {
            return Err(vec![LoweringIssue::UnsupportedType { node: 0 }]);
        };
        let value = next_value;
        next_value += 1;
        program.values.insert(
            value,
            MirValue {
                id: value,
                ty,
                location: None,
            },
        );
        graph_inputs.insert(input.id, value);
        program.inputs.push(value);
    }

    let mut issues = Vec::new();

    for node_id in order {
        let node = graph
            .nodes
            .iter()
            .find(|node| node.id == node_id)
            .expect("validated node");

        let mut inputs = Vec::with_capacity(node.inputs.len());
        for input in &node.inputs {
            match find_input_value(
                graph,
                node.id,
                input.id,
                &output_values,
                &graph_inputs,
            ) {
                Some(value) => inputs.push(value),
                None => issues.push(LoweringIssue::MissingInputValue {
                    node: node.id,
                    port: input.id,
                }),
            }
        }

        let mir_op = match &node.operation {
            Operation::Const(Literal::Integer(value)) => {
                Some(MirOp::ConstInteger(*value))
            }
            Operation::Const(Literal::Bool(value)) => Some(MirOp::ConstBool(*value)),
            Operation::Add => Some(MirOp::Add {
                mode: ArithmeticMode::Checked,
            }),
            Operation::Sub => Some(MirOp::Sub {
                mode: ArithmeticMode::Checked,
            }),
            Operation::Mul => Some(MirOp::Mul {
                mode: ArithmeticMode::Checked,
            }),
            Operation::Eq
            | Operation::Lt
            | Operation::Le
            | Operation::Gt
            | Operation::Ge => {
                let Some(SemanticType::Integer(range)) =
                    node.inputs.first().map(|port| &port.ty)
                else {
                    issues.push(LoweringIssue::UnsupportedType {
                        node: node.id,
                    });
                    continue;
                };
                let kind = match node.operation {
                    Operation::Eq => CompareKind::Eq,
                    Operation::Lt => CompareKind::Lt,
                    Operation::Le => CompareKind::Le,
                    Operation::Gt => CompareKind::Gt,
                    Operation::Ge => CompareKind::Ge,
                    _ => unreachable!(),
                };
                Some(MirOp::Compare {
                    kind,
                    width: choose_integer_width(range),
                    signed: range.min < 0,
                })
            }
            Operation::And | Operation::Or | Operation::Xor => {
                let kind = match node.operation {
                    Operation::And => BoolBinaryKind::And,
                    Operation::Or => BoolBinaryKind::Or,
                    Operation::Xor => BoolBinaryKind::Xor,
                    _ => unreachable!(),
                };
                Some(MirOp::BoolBinary { kind })
            }
            Operation::Not => Some(MirOp::BoolNot),
            Operation::Subgraph(target) => Some(MirOp::Call {
                target: target.clone(),
            }),
            Operation::Select {
                when_true,
                when_false,
            } => Some(MirOp::SelectCall {
                when_true: when_true.clone(),
                when_false: when_false.clone(),
            }),
            Operation::Loop {
                condition,
                body,
                max_iterations,
            } => Some(MirOp::LoopCall {
                condition: condition.clone(),
                body: body.clone(),
                max_iterations: *max_iterations,
            }),
            operation => {
                issues.push(LoweringIssue::UnsupportedOperation {
                    node: node.id,
                    operation: format!("{operation:?}"),
                });
                None
            }
        };

        let mut outputs = Vec::with_capacity(node.outputs.len());
        for output in &node.outputs {
            let Some(ty) = lower_type(&output.ty) else {
                issues.push(LoweringIssue::UnsupportedType { node: node.id });
                continue;
            };
            let value = next_value;
            next_value += 1;
            program.values.insert(
                value,
                MirValue {
                    id: value,
                    ty,
                    location: None,
                },
            );
            output_values.insert((node.id, output.id), value);
            outputs.push(value);
        }

        if let Some(op) = mir_op {
            program.instructions.push(MirInstruction {
                source_node: node.id,
                op,
                inputs,
                outputs,
            });
        }
    }

    for graph_output in &graph.outputs {
        let value = graph.edges.iter().find_map(|edge| {
            match (&edge.from, &edge.to) {
                (
                    SourceEndpoint::NodeOutput {
                        node,
                        port,
                    },
                    TargetEndpoint::GraphOutput(output_port),
                ) if *output_port == graph_output.id => {
                    output_values.get(&(*node, *port)).copied()
                }
                (
                    SourceEndpoint::GraphInput(input_port),
                    TargetEndpoint::GraphOutput(output_port),
                ) if *output_port == graph_output.id => {
                    graph_inputs.get(input_port).copied()
                }
                _ => None,
            }
        });

        match value {
            Some(value) => program.outputs.push(value),
            None => issues.push(LoweringIssue::MissingInputValue {
                node: 0,
                port: graph_output.id,
            }),
        }
    }

    if issues.is_empty() {
        Ok(program)
    } else {
        Err(issues)
    }
}

fn lower_type(ty: &SemanticType) -> Option<MirType> {
    match ty {
        SemanticType::Bool => Some(MirType::Bool),
        SemanticType::Integer(range) => {
            Some(MirType::Integer(choose_integer_width(range)))
        }
        SemanticType::Float(_) => Some(MirType::Float64),
        SemanticType::Bytes => Some(MirType::Bytes),
        SemanticType::Text => Some(MirType::TextHandle),
        SemanticType::Reference(_)
        | SemanticType::Unique(_)
        | SemanticType::Borrow(_)
        | SemanticType::Shared(_)
        | SemanticType::State(_)
        | SemanticType::Atomic(_)
        | SemanticType::Versioned(_) => Some(MirType::Pointer),
        SemanticType::BigInteger
        | SemanticType::Rational
        | SemanticType::Decimal(_)
        | SemanticType::BigFloat(_)
        | SemanticType::Array(_, _)
        | SemanticType::Slice(_)
        | SemanticType::Vector(_, _)
        | SemanticType::Record(_)
        | SemanticType::Variant(_)
        | SemanticType::Option(_)
        | SemanticType::Result(_, _)
        | SemanticType::Secret(_)
        | SemanticType::Credential(_) => None,
    }
}

fn find_input_value(
    graph: &Graph,
    node: NodeId,
    port: u16,
    outputs: &BTreeMap<(NodeId, u16), ValueId>,
    graph_inputs: &BTreeMap<u16, ValueId>,
) -> Option<ValueId> {
    graph.edges.iter().find_map(|edge| match (&edge.from, &edge.to) {
        (
            SourceEndpoint::NodeOutput {
                node: from_node,
                port: from_port,
            },
            TargetEndpoint::NodeInput {
                node: to_node,
                port: to_port,
            },
        ) if *to_node == node && *to_port == port => {
            outputs.get(&(*from_node, *from_port)).copied()
        }
        (
            SourceEndpoint::GraphInput(from_port),
            TargetEndpoint::NodeInput {
                node: to_node,
                port: to_port,
            },
        ) if *to_node == node && *to_port == port => {
            graph_inputs.get(from_port).copied()
        }
        _ => None,
    })
}

fn topological_order(graph: &Graph) -> Option<Vec<NodeId>> {
    let nodes: BTreeSet<NodeId> =
        graph.nodes.iter().map(|node| node.id).collect();
    let mut indegree: BTreeMap<NodeId, usize> =
        nodes.iter().map(|node| (*node, 0)).collect();
    let mut adjacency: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();

    for edge in &graph.edges {
        if let (
            SourceEndpoint::NodeOutput { node: from, .. },
            TargetEndpoint::NodeInput { node: to, .. },
        ) = (&edge.from, &edge.to)
        {
            adjacency.entry(*from).or_default().push(*to);
            *indegree.entry(*to).or_default() += 1;
        }
    }

    let mut ready: BTreeSet<NodeId> = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree == 0).then_some(*node))
        .collect();
    let mut order = Vec::with_capacity(nodes.len());

    while let Some(node) = ready.pop_first() {
        order.push(node);
        if let Some(children) = adjacency.get(&node) {
            for child in children {
                let degree = indegree.get_mut(child).expect("known node");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(*child);
                }
            }
        }
    }

    (order.len() == nodes.len()).then_some(order)
}

pub fn integer_type_from_range(range: &IntegerType) -> MirType {
    MirType::Integer(choose_integer_width(range))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{
        AuthorityMode, Edge, Node, Port, TargetEndpoint,
    };

    fn int(min: i128, max: i128) -> SemanticType {
        SemanticType::Integer(IntegerType::new(min, max).unwrap())
    }

    #[test]
    fn range_lowering_chooses_compact_physical_integer() {
        assert_eq!(
            integer_type_from_range(&IntegerType::new(0, 255).unwrap()),
            MirType::Integer(IntegerWidth::U8)
        );
        assert_eq!(
            integer_type_from_range(&IntegerType::new(-1000, 1000).unwrap()),
            MirType::Integer(IntegerWidth::I16)
        );
    }

    #[test]
    fn lowers_checked_integer_add_in_topological_order() {
        let graph = Graph {
            name: "add".into(),
            inputs: vec![],
            outputs: vec![],
            nodes: vec![
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
                    operation: Operation::Add,
                    inputs: vec![
                        Port {
                            id: 0,
                            name: "a".into(),
                            ty: int(20, 20),
                        },
                        Port {
                            id: 1,
                            name: "b".into(),
                            ty: int(22, 22),
                        },
                    ],
                    outputs: vec![Port {
                        id: 0,
                        name: "sum".into(),
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
        };

        let mir = lower_graph(&graph).unwrap();
        assert_eq!(mir.instructions.len(), 3);
        assert!(matches!(
            mir.instructions[2].op,
            MirOp::Add {
                mode: ArithmeticMode::Checked
            }
        ));
        assert_eq!(
            mir.values[&mir.instructions[2].outputs[0]].ty,
            MirType::Integer(IntegerWidth::U8)
        );
    }
}
