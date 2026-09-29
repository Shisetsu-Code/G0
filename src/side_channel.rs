use crate::gir::{Graph, NodeId, Operation, SemanticType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SideChannelIssue {
    SensitiveBranch(NodeId),
    SensitiveMatch(NodeId),
    SensitiveLoop(NodeId),
}

pub fn validate_side_channels(
    graph: &Graph,
) -> Result<(), Vec<SideChannelIssue>> {
    let mut issues = Vec::new();

    for node in &graph.nodes {
        let sensitive_input = node
            .inputs
            .iter()
            .any(|port| is_sensitive(&port.ty));

        if !sensitive_input {
            continue;
        }

        match node.operation {
            Operation::Select { .. } => {
                issues.push(SideChannelIssue::SensitiveBranch(node.id));
            }
            Operation::Match { .. } => {
                issues.push(SideChannelIssue::SensitiveMatch(node.id));
            }
            Operation::Loop { .. } => {
                issues.push(SideChannelIssue::SensitiveLoop(node.id));
            }
            _ => {}
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn is_sensitive(ty: &SemanticType) -> bool {
    match ty {
        SemanticType::Secret(_) | SemanticType::Credential(_) => true,
        SemanticType::Array(inner, _)
        | SemanticType::Slice(inner)
        | SemanticType::Vector(inner, _)
        | SemanticType::Option(inner)
        | SemanticType::Unique(inner)
        | SemanticType::Borrow(inner)
        | SemanticType::Shared(inner)
        | SemanticType::State(inner)
        | SemanticType::Atomic(inner)
        | SemanticType::Versioned(inner) => is_sensitive(inner),
        SemanticType::Result(ok, error) => is_sensitive(ok) || is_sensitive(error),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{Node, Port};

    #[test]
    fn secret_cannot_directly_control_branch() {
        let mut graph = Graph::new("auth");
        graph.nodes.push(Node {
            id: 1,
            operation: Operation::Select {
                when_true: "yes".into(),
                when_false: "no".into(),
            },
            inputs: vec![Port {
                id: 0,
                name: "condition".into(),
                ty: SemanticType::Secret(Box::new(SemanticType::Bool)),
            }],
            outputs: vec![],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        });

        assert_eq!(
            validate_side_channels(&graph),
            Err(vec![SideChannelIssue::SensitiveBranch(1)])
        );
    }

    #[test]
    fn public_control_flow_is_allowed() {
        let mut graph = Graph::new("public");
        graph.nodes.push(Node {
            id: 1,
            operation: Operation::Select {
                when_true: "yes".into(),
                when_false: "no".into(),
            },
            inputs: vec![Port {
                id: 0,
                name: "condition".into(),
                ty: SemanticType::Bool,
            }],
            outputs: vec![],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        });

        assert!(validate_side_channels(&graph).is_ok());
    }
}
