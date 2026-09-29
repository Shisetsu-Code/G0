use std::collections::{BTreeMap, BTreeSet};

use crate::gir::{Graph, NodeId, SourceEndpoint, TargetEndpoint};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipMode {
    Value,
    Unique,
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UseKind {
    Read,
    BorrowShared,
    BorrowMutable,
    Move,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedValue {
    pub id: String,
    pub mode: OwnershipMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueUse {
    pub value: String,
    pub node: NodeId,
    pub kind: UseKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OwnershipPlan {
    pub values: Vec<OwnedValue>,
    pub uses: Vec<ValueUse>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnershipIssue {
    DuplicateValue(String),
    UnknownValue(String),
    UnknownNode(NodeId),
    MultipleMoves {
        value: String,
        nodes: BTreeSet<NodeId>,
    },
    UseNotBeforeMove {
        value: String,
        move_node: NodeId,
        use_node: NodeId,
    },
    ConflictingBorrow {
        value: String,
        first: NodeId,
        second: NodeId,
    },
    MoveFromShared(String),
}

pub fn validate_ownership(
    graph: &Graph,
    plan: &OwnershipPlan,
) -> Result<(), Vec<OwnershipIssue>> {
    let mut issues = Vec::new();
    let node_ids: BTreeSet<NodeId> =
        graph.nodes.iter().map(|node| node.id).collect();
    let mut values = BTreeMap::<&str, OwnershipMode>::new();

    for value in &plan.values {
        if values.insert(value.id.as_str(), value.mode).is_some() {
            issues.push(OwnershipIssue::DuplicateValue(value.id.clone()));
        }
    }

    let reachability = Reachability::new(graph);
    let mut uses_by_value: BTreeMap<&str, Vec<&ValueUse>> = BTreeMap::new();

    for usage in &plan.uses {
        if !values.contains_key(usage.value.as_str()) {
            issues.push(OwnershipIssue::UnknownValue(usage.value.clone()));
            continue;
        }
        if !node_ids.contains(&usage.node) {
            issues.push(OwnershipIssue::UnknownNode(usage.node));
            continue;
        }
        uses_by_value
            .entry(usage.value.as_str())
            .or_default()
            .push(usage);
    }

    for (value, uses) in uses_by_value {
        let mode = values[value];
        let moves: Vec<&ValueUse> = uses
            .iter()
            .copied()
            .filter(|usage| usage.kind == UseKind::Move)
            .collect();

        if mode == OwnershipMode::Shared && !moves.is_empty() {
            issues.push(OwnershipIssue::MoveFromShared(value.to_owned()));
        }

        if moves.len() > 1 {
            issues.push(OwnershipIssue::MultipleMoves {
                value: value.to_owned(),
                nodes: moves.iter().map(|usage| usage.node).collect(),
            });
        }

        if let Some(move_use) = moves.first() {
            for usage in &uses {
                if usage.node == move_use.node && usage.kind == UseKind::Move {
                    continue;
                }
                if !reachability.before(usage.node, move_use.node) {
                    issues.push(OwnershipIssue::UseNotBeforeMove {
                        value: value.to_owned(),
                        move_node: move_use.node,
                        use_node: usage.node,
                    });
                }
            }
        }

        for (index, first) in uses.iter().enumerate() {
            for second in uses.iter().skip(index + 1) {
                if borrows_conflict(first.kind, second.kind)
                    && reachability.may_overlap(first.node, second.node)
                {
                    issues.push(OwnershipIssue::ConflictingBorrow {
                        value: value.to_owned(),
                        first: first.node,
                        second: second.node,
                    });
                }
            }
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn borrows_conflict(a: UseKind, b: UseKind) -> bool {
    match (a, b) {
        (UseKind::BorrowMutable, UseKind::BorrowMutable)
        | (UseKind::BorrowMutable, UseKind::BorrowShared)
        | (UseKind::BorrowShared, UseKind::BorrowMutable)
        | (UseKind::BorrowMutable, UseKind::Read)
        | (UseKind::Read, UseKind::BorrowMutable)
        | (UseKind::Move, _)
        | (_, UseKind::Move) => true,
        (UseKind::Read, UseKind::Read)
        | (UseKind::Read, UseKind::BorrowShared)
        | (UseKind::BorrowShared, UseKind::Read)
        | (UseKind::BorrowShared, UseKind::BorrowShared) => false,
    }
}

struct Reachability {
    closure: BTreeMap<NodeId, BTreeSet<NodeId>>,
}

impl Reachability {
    fn new(graph: &Graph) -> Self {
        let mut adjacency = BTreeMap::<NodeId, Vec<NodeId>>::new();
        for edge in &graph.edges {
            if let (
                SourceEndpoint::NodeOutput { node: from, .. },
                TargetEndpoint::NodeInput { node: to, .. },
            ) = (&edge.from, &edge.to)
            {
                adjacency.entry(*from).or_default().push(*to);
            }
        }

        let mut closure = BTreeMap::new();
        for node in graph.nodes.iter().map(|node| node.id) {
            let mut seen = BTreeSet::new();
            let mut stack = vec![node];
            while let Some(current) = stack.pop() {
                if let Some(children) = adjacency.get(&current) {
                    for child in children {
                        if seen.insert(*child) {
                            stack.push(*child);
                        }
                    }
                }
            }
            closure.insert(node, seen);
        }

        Self { closure }
    }

    fn before(&self, first: NodeId, second: NodeId) -> bool {
        self.closure
            .get(&first)
            .is_some_and(|reachable| reachable.contains(&second))
    }

    fn may_overlap(&self, a: NodeId, b: NodeId) -> bool {
        a != b && !self.before(a, b) && !self.before(b, a)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{
        AuthorityMode, Edge, Node, Operation, Port, SemanticType,
    };

    fn node(id: NodeId) -> Node {
        Node {
            id,
            operation: Operation::Subgraph(format!("n{id}")),
            inputs: vec![Port {
                id: 0,
                name: "in".into(),
                ty: SemanticType::Bytes,
            }],
            outputs: vec![Port {
                id: 0,
                name: "out".into(),
                ty: SemanticType::Bytes,
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        }
    }

    fn graph_with_edge(from: NodeId, to: NodeId) -> Graph {
        Graph {
            name: "ownership".into(),
            inputs: vec![],
            outputs: vec![],
            nodes: vec![node(1), node(2), node(3)],
            edges: vec![Edge {
                from: SourceEndpoint::NodeOutput {
                    node: from,
                    port: 0,
                },
                to: TargetEndpoint::NodeInput {
                    node: to,
                    port: 0,
                },
            }],
            authority: AuthorityMode::DefaultDeny,
        }
    }

    #[test]
    fn reads_before_move_are_valid_when_dependency_proves_order() {
        let graph = graph_with_edge(1, 2);
        let plan = OwnershipPlan {
            values: vec![OwnedValue {
                id: "buffer".into(),
                mode: OwnershipMode::Unique,
            }],
            uses: vec![
                ValueUse {
                    value: "buffer".into(),
                    node: 1,
                    kind: UseKind::Read,
                },
                ValueUse {
                    value: "buffer".into(),
                    node: 2,
                    kind: UseKind::Move,
                },
            ],
        };

        assert!(validate_ownership(&graph, &plan).is_ok());
    }

    #[test]
    fn unordered_use_and_move_are_rejected() {
        let graph = graph_with_edge(1, 2);
        let plan = OwnershipPlan {
            values: vec![OwnedValue {
                id: "buffer".into(),
                mode: OwnershipMode::Unique,
            }],
            uses: vec![
                ValueUse {
                    value: "buffer".into(),
                    node: 2,
                    kind: UseKind::Move,
                },
                ValueUse {
                    value: "buffer".into(),
                    node: 3,
                    kind: UseKind::Read,
                },
            ],
        };

        assert!(validate_ownership(&graph, &plan)
            .unwrap_err()
            .iter()
            .any(|issue| {
                matches!(
                    issue,
                    OwnershipIssue::UseNotBeforeMove {
                        use_node: 3,
                        ..
                    }
                )
            }));
    }

    #[test]
    fn concurrent_mutable_and_shared_borrows_are_rejected() {
        let graph = graph_with_edge(1, 2);
        let plan = OwnershipPlan {
            values: vec![OwnedValue {
                id: "state".into(),
                mode: OwnershipMode::Unique,
            }],
            uses: vec![
                ValueUse {
                    value: "state".into(),
                    node: 1,
                    kind: UseKind::BorrowMutable,
                },
                ValueUse {
                    value: "state".into(),
                    node: 3,
                    kind: UseKind::BorrowShared,
                },
            ],
        };

        assert!(validate_ownership(&graph, &plan)
            .unwrap_err()
            .iter()
            .any(|issue| {
                matches!(issue, OwnershipIssue::ConflictingBorrow { .. })
            }));
    }

    #[test]
    fn concurrent_shared_borrows_are_valid() {
        let graph = graph_with_edge(1, 2);
        let plan = OwnershipPlan {
            values: vec![OwnedValue {
                id: "data".into(),
                mode: OwnershipMode::Unique,
            }],
            uses: vec![
                ValueUse {
                    value: "data".into(),
                    node: 1,
                    kind: UseKind::BorrowShared,
                },
                ValueUse {
                    value: "data".into(),
                    node: 3,
                    kind: UseKind::BorrowShared,
                },
            ],
        };

        assert!(validate_ownership(&graph, &plan).is_ok());
    }
}
