use std::collections::{BTreeSet, VecDeque};

use crate::gir::{
    Graph, Literal, NodeId, Operation, SourceEndpoint, TargetEndpoint,
};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GirOptimizationReport {
    pub folded_nodes: BTreeSet<NodeId>,
    pub removed_nodes: BTreeSet<NodeId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GirOptimizationIssue {
    InputNotConstant(NodeId),
    ArithmeticOverflow(NodeId),
    ResultOutOfDeclaredRange(NodeId),
    InvalidResultGraph,
}

pub fn optimize_gir(
    graph: &Graph,
) -> Result<(Graph, GirOptimizationReport), Vec<GirOptimizationIssue>> {
    if crate::gir_validate::validate(graph).is_err() {
        return Err(vec![GirOptimizationIssue::InvalidResultGraph]);
    }

    let mut optimized = graph.clone();
    let mut report = GirOptimizationReport::default();

    fold_constants(&mut optimized, &mut report)?;
    eliminate_dead_pure_nodes(&mut optimized, &mut report);

    if crate::gir_validate::validate(&optimized).is_err() {
        return Err(vec![GirOptimizationIssue::InvalidResultGraph]);
    }

    Ok((optimized, report))
}

fn fold_constants(
    graph: &mut Graph,
    report: &mut GirOptimizationReport,
) -> Result<(), Vec<GirOptimizationIssue>> {
    loop {
        let mut changed = false;
        let snapshot = graph.clone();

        for node_index in 0..graph.nodes.len() {
            let node = &snapshot.nodes[node_index];
            let arithmetic = matches!(
                node.operation,
                Operation::Add
                    | Operation::Sub
                    | Operation::Mul
                    | Operation::Div
                    | Operation::Rem
            );
            if !arithmetic {
                continue;
            }

            let Some((left, right)) = constant_integer_inputs(&snapshot, node.id)
            else {
                continue;
            };

            let value = match node.operation {
                Operation::Add => left.checked_add(right),
                Operation::Sub => left.checked_sub(right),
                Operation::Mul => left.checked_mul(right),
                Operation::Div => left.checked_div(right),
                Operation::Rem => left.checked_rem(right),
                _ => unreachable!(),
            };

            let Some(value) = value else {
                if matches!(node.operation, Operation::Div | Operation::Rem) {
                    // Preserve checked runtime trap semantics (e.g. divisor zero).
                    continue;
                }
                return Err(vec![
                    GirOptimizationIssue::ArithmeticOverflow(node.id),
                ]);
            };

            let Some(output) = node.outputs.first() else {
                continue;
            };
            let crate::gir::SemanticType::Integer(range) = &output.ty else {
                continue;
            };

            if value < range.min || value > range.max {
                return Err(vec![
                    GirOptimizationIssue::ResultOutOfDeclaredRange(node.id),
                ]);
            }

            let target = &mut graph.nodes[node_index];
            target.operation = Operation::Const(Literal::Integer(value));
            target.inputs.clear();
            graph.edges.retain(|edge| {
                !matches!(
                    edge.to,
                    TargetEndpoint::NodeInput { node: id, .. } if id == node.id
                )
            });
            report.folded_nodes.insert(node.id);
            changed = true;
        }

        if !changed {
            break;
        }
    }

    Ok(())
}

fn constant_integer_inputs(
    graph: &Graph,
    node: NodeId,
) -> Option<(i128, i128)> {
    let target = graph.nodes.iter().find(|item| item.id == node)?;
    if target.inputs.len() != 2 {
        return None;
    }

    let mut values = Vec::new();
    for input in &target.inputs {
        let edge = graph.edges.iter().find(|edge| {
            matches!(
                edge.to,
                TargetEndpoint::NodeInput {
                    node: id,
                    port
                } if id == node && port == input.id
            )
        })?;

        let SourceEndpoint::NodeOutput {
            node: source,
            ..
        } = edge.from
        else {
            return None;
        };
        let source_node = graph.nodes.iter().find(|item| item.id == source)?;
        let Operation::Const(Literal::Integer(value)) = source_node.operation
        else {
            return None;
        };
        values.push(value);
    }

    Some((values[0], values[1]))
}

fn eliminate_dead_pure_nodes(
    graph: &mut Graph,
    report: &mut GirOptimizationReport,
) {
    let mut live = BTreeSet::new();
    let mut queue = VecDeque::new();

    for edge in &graph.edges {
        if matches!(edge.to, TargetEndpoint::GraphOutput(_))
            && let SourceEndpoint::NodeOutput { node, .. } = edge.from
            && live.insert(node)
        {
            queue.push_back(node);
        }
    }

    for node in &graph.nodes {
        if !node.effects.is_empty() && live.insert(node.id) {
            queue.push_back(node.id);
        }
    }

    while let Some(node_id) = queue.pop_front() {
        for edge in &graph.edges {
            if matches!(
                edge.to,
                TargetEndpoint::NodeInput { node, .. } if node == node_id
            ) && let SourceEndpoint::NodeOutput { node: source, .. } = edge.from
                && live.insert(source)
            {
                queue.push_back(source);
            }
        }
    }

    let all: BTreeSet<NodeId> =
        graph.nodes.iter().map(|node| node.id).collect();
    let removed: BTreeSet<NodeId> =
        all.difference(&live).copied().collect();

    if removed.is_empty() {
        return;
    }

    graph.nodes.retain(|node| !removed.contains(&node.id));
    graph.edges.retain(|edge| {
        let source_removed = matches!(
            edge.from,
            SourceEndpoint::NodeOutput { node, .. } if removed.contains(&node)
        );
        let target_removed = matches!(
            edge.to,
            TargetEndpoint::NodeInput { node, .. } if removed.contains(&node)
        );
        !source_removed && !target_removed
    });

    report.removed_nodes.extend(removed);
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{
        AuthorityMode, Edge, IntegerType, Node, Port, SemanticType,
    };

    fn int(min: i128, max: i128) -> SemanticType {
        SemanticType::Integer(IntegerType::new(min, max).unwrap())
    }

    #[test]
    fn folds_constant_graph_and_removes_dead_inputs() {
        let graph = Graph {
            name: "fold".into(),
            inputs: vec![],
            outputs: vec![Port {
                id: 0,
                name: "out".into(),
                ty: int(42, 42),
            }],
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
                Edge {
                    from: SourceEndpoint::NodeOutput { node: 3, port: 0 },
                    to: TargetEndpoint::GraphOutput(0),
                },
            ],
            authority: AuthorityMode::DefaultDeny,
        };

        let (optimized, report) = optimize_gir(&graph).unwrap();
        assert!(report.folded_nodes.contains(&3));
        assert!(report.removed_nodes.contains(&1));
        assert!(report.removed_nodes.contains(&2));
        assert_eq!(optimized.nodes.len(), 1);
        assert!(matches!(
            optimized.nodes[0].operation,
            Operation::Const(Literal::Integer(42))
        ));
    }

    #[test]
    fn effectful_dead_node_is_never_removed() {
        let mut graph = Graph::new("effect");
        let mut effects = BTreeSet::new();
        effects.insert(crate::gir::Effect::Audit);
        let mut caps = BTreeSet::new();
        caps.insert(crate::gir::Capability::new(
            crate::gir::CapabilityClass::Audit,
            "record",
            "Audit",
            "current",
        ));
        graph.nodes.push(Node {
            id: 1,
            operation: Operation::Subgraph("audit".into()),
            inputs: vec![],
            outputs: vec![],
            effects,
            required_capabilities: caps,
        });

        let (optimized, report) = optimize_gir(&graph).unwrap();
        assert!(report.removed_nodes.is_empty());
        assert_eq!(optimized.nodes.len(), 1);
    }
}
