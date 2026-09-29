use std::collections::BTreeMap;

use crate::gir::NodeId;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum InvariantKind {
    IntegerRange,
    TypeSafety,
    GraphAcyclic,
    Ownership,
    RaceFreedom,
    CapabilityPresent,
    Authorization,
    InformationFlow,
    MemoryBounds,
    RetrySafety,
    Determinism,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvariantStatus {
    Proven,
    RuntimeCheckRequired,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InvariantKey {
    pub graph: String,
    pub node: Option<NodeId>,
    pub kind: InvariantKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InvariantLedger {
    pub entries: BTreeMap<InvariantKey, InvariantStatus>,
}

impl InvariantLedger {
    pub fn record(&mut self, key: InvariantKey, status: InvariantStatus) {
        self.entries.insert(key, status);
    }

    pub fn compilation_allowed(&self) -> bool {
        self.entries
            .values()
            .all(|status| *status != InvariantStatus::Invalid)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreservationIssue {
    MissingInvariant(InvariantKey),
    BecameInvalid(InvariantKey),
}

pub fn validate_preservation(
    before: &InvariantLedger,
    after: &InvariantLedger,
) -> Result<(), Vec<PreservationIssue>> {
    let mut issues = Vec::new();

    for (key, previous) in &before.entries {
        let Some(current) = after.entries.get(key) else {
            issues.push(PreservationIssue::MissingInvariant(key.clone()));
            continue;
        };

        if *previous != InvariantStatus::Invalid
            && *current == InvariantStatus::Invalid
        {
            issues.push(PreservationIssue::BecameInvalid(key.clone()));
        }
    }

    if issues.is_empty() { Ok(()) } else { Err(issues) }
}

pub fn ledger_for_validated_graph(
    graph: &crate::gir::Graph,
) -> InvariantLedger {
    let mut ledger = InvariantLedger::default();
    ledger.record(
        InvariantKey {
            graph: graph.name.clone(),
            node: None,
            kind: InvariantKind::GraphAcyclic,
        },
        InvariantStatus::Proven,
    );

    for node in &graph.nodes {
        ledger.record(
            InvariantKey {
                graph: graph.name.clone(),
                node: Some(node.id),
                kind: InvariantKind::TypeSafety,
            },
            InvariantStatus::Proven,
        );

        if node.outputs.iter().any(|port| {
            matches!(port.ty, crate::gir::SemanticType::Integer(_))
        }) {
            ledger.record(
                InvariantKey {
                    graph: graph.name.clone(),
                    node: Some(node.id),
                    kind: InvariantKind::IntegerRange,
                },
                InvariantStatus::Proven,
            );
        }
    }

    ledger
}

pub fn validate_preservation_with_retired_nodes(
    before: &InvariantLedger,
    after: &InvariantLedger,
    retired_nodes: &std::collections::BTreeSet<NodeId>,
) -> Result<(), Vec<PreservationIssue>> {
    let mut filtered = before.clone();
    filtered.entries.retain(|key, _| {
        key.node
            .is_none_or(|node| !retired_nodes.contains(&node))
    });
    validate_preservation(&filtered, after)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transformation_cannot_drop_invariant() {
        let key = InvariantKey {
            graph: "main".into(),
            node: Some(1),
            kind: InvariantKind::IntegerRange,
        };
        let mut before = InvariantLedger::default();
        before.record(key.clone(), InvariantStatus::Proven);

        assert_eq!(
            validate_preservation(&before, &InvariantLedger::default()),
            Err(vec![PreservationIssue::MissingInvariant(key)])
        );
    }
}
