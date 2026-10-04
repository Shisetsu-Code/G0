use std::collections::{BTreeMap, BTreeSet};

use crate::gir::{
    Capability, Effect, Graph, MatchArm, Node, NodeId, Operation, Port, SemanticType,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlIssue {
    UnknownGraph {
        owner: String,
        node: NodeId,
        target: String,
    },
    SelectMissingCondition {
        owner: String,
        node: NodeId,
    },
    SelectConditionNotBool {
        owner: String,
        node: NodeId,
    },
    MatchMissingSelector {
        owner: String,
        node: NodeId,
    },
    MatchSelectorNotVariant {
        owner: String,
        node: NodeId,
    },
    EmptyMatchTag {
        owner: String,
        node: NodeId,
    },
    DuplicateMatchTag {
        owner: String,
        node: NodeId,
        tag: String,
    },
    ZeroLoopBound {
        owner: String,
        node: NodeId,
    },
    BranchInputMismatch {
        owner: String,
        node: NodeId,
        target: String,
    },
    BranchOutputMismatch {
        owner: String,
        node: NodeId,
        target: String,
    },
    LoopStateMismatch {
        owner: String,
        node: NodeId,
    },
    LoopConditionOutputInvalid {
        owner: String,
        node: NodeId,
        condition: String,
    },
    LoopConditionHasEffects {
        owner: String,
        node: NodeId,
        condition: String,
    },
    EffectSummaryMismatch {
        owner: String,
        node: NodeId,
    },
    ControlReferenceCycle(Vec<String>),
}

pub fn validate_control_graphs(graphs: &[Graph]) -> Result<(), Vec<ControlIssue>> {
    let by_name: BTreeMap<&str, &Graph> = graphs
        .iter()
        .map(|graph| (graph.name.as_str(), graph))
        .collect();
    let mut issues = Vec::new();
    let mut references = BTreeMap::<String, BTreeSet<String>>::new();

    for graph in graphs {
        for node in &graph.nodes {
            match &node.operation {
                Operation::TaskSpawn { body, .. }
                | Operation::TaskJoin { body }
                | Operation::TaskSpawnScoped { body, .. }
                | Operation::TaskJoinScoped { body } => {
                    validate_task(graph, node, body, &by_name, &mut issues);
                    references
                        .entry(graph.name.clone())
                        .or_default()
                        .insert(body.clone());
                }
                Operation::Map { body } => {
                    validate_map(graph, node, body, &by_name, &mut issues);
                    references
                        .entry(graph.name.clone())
                        .or_default()
                        .insert(body.clone());
                }
                Operation::Select {
                    when_true,
                    when_false,
                } => {
                    validate_select(graph, node, when_true, when_false, &by_name, &mut issues);
                    references
                        .entry(graph.name.clone())
                        .or_default()
                        .extend([when_true.clone(), when_false.clone()]);
                }
                Operation::Match { arms, default } => {
                    validate_match(graph, node, arms, default, &by_name, &mut issues);
                    let entry = references.entry(graph.name.clone()).or_default();
                    entry.insert(default.clone());
                    entry.extend(arms.iter().map(|arm| arm.graph.clone()));
                }
                Operation::Loop {
                    condition,
                    body,
                    max_iterations,
                } => {
                    validate_loop(
                        graph,
                        node,
                        condition,
                        body,
                        *max_iterations,
                        &by_name,
                        &mut issues,
                    );
                    references
                        .entry(graph.name.clone())
                        .or_default()
                        .extend([condition.clone(), body.clone()]);
                }
                _ => {}
            }
        }
    }

    if issues.is_empty()
        && let Some(cycle) = find_cycle(&references, &by_name)
    {
        issues.push(ControlIssue::ControlReferenceCycle(cycle));
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn control_references(graph: &Graph) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    for node in &graph.nodes {
        match &node.operation {
            Operation::TaskSpawn { body, .. }
            | Operation::TaskJoin { body }
            | Operation::TaskSpawnScoped { body, .. }
            | Operation::TaskJoinScoped { body } => {
                result.insert(body.clone());
            }
            Operation::Map { body } => {
                result.insert(body.clone());
            }
            Operation::Select {
                when_true,
                when_false,
            } => {
                result.insert(when_true.clone());
                result.insert(when_false.clone());
            }
            Operation::Match { arms, default } => {
                result.insert(default.clone());
                result.extend(arms.iter().map(|arm| arm.graph.clone()));
            }
            Operation::Loop {
                condition, body, ..
            } => {
                result.insert(condition.clone());
                result.insert(body.clone());
            }
            _ => {}
        }
    }
    result
}

fn validate_task(
    owner: &Graph,
    node: &Node,
    target: &str,
    by_name: &BTreeMap<&str, &Graph>,
    issues: &mut Vec<ControlIssue>,
) {
    let Some(body) = by_name.get(target).copied() else {
        issues.push(ControlIssue::UnknownGraph {
            owner: owner.name.clone(),
            node: node.id,
            target: target.into(),
        });
        return;
    };
    let scoped = matches!(
        node.operation,
        Operation::TaskSpawnScoped { .. } | Operation::TaskJoinScoped { .. }
    );
    let kind = if scoped { "g0.scoped-task" } else { "g0.task" };
    let task_type = SemanticType::Unique(Box::new(SemanticType::Reference(format!(
        "{kind}:{target}"
    ))));
    let (valid, action) = match &node.operation {
        Operation::TaskSpawn {
            max_steps,
            max_value_bytes,
            ..
        }
        | Operation::TaskSpawnScoped {
            max_steps,
            max_value_bytes,
            ..
        } => (
            *max_steps > 0
                && *max_value_bytes > 0
                && sequenced_interface(&node.inputs, &body.inputs)
                && (1..=2).contains(&node.outputs.len())
                && node.outputs[0].ty == task_type
                && (node.outputs.len() == 1 || node.outputs[1].ty == SemanticType::Bool),
            if scoped { "spawn-scoped" } else { "spawn" },
        ),
        Operation::TaskJoin { .. } | Operation::TaskJoinScoped { .. } => (
            (1..=2).contains(&node.inputs.len())
                && node.inputs[0].ty == task_type
                && (node.inputs.len() == 1 || node.inputs[1].ty == SemanticType::Bool)
                && sequenced_interface(&node.outputs, &body.outputs),
            if scoped { "join-scoped" } else { "join" },
        ),
        _ => return,
    };
    let cap = Capability::new(
        crate::gir::CapabilityClass::LocalExecution,
        action,
        target,
        "tasks",
    );
    let (effects, capabilities) = graph_effect_summary(body);
    let mut expected_effects = effects.clone();
    expected_effects.insert(Effect::LocalExecution);
    if !valid
        || (if scoped {
            effects.iter().any(|e| *e != Effect::MemoryWrite)
        } else {
            !effects.is_empty()
        })
        || !capabilities.is_empty()
        || node.effects != expected_effects
        || node.required_capabilities != BTreeSet::from([cap])
    {
        issues.push(ControlIssue::EffectSummaryMismatch {
            owner: owner.name.clone(),
            node: node.id,
        });
    }
}

fn sequenced_interface(actual: &[Port], payload: &[Port]) -> bool {
    (actual.len() == payload.len() || actual.len() == payload.len() + 1)
        && same_interface(&actual[..payload.len()], payload)
        && (actual.len() == payload.len()
            || actual.last().is_some_and(|p| p.ty == SemanticType::Bool))
}

fn validate_map(
    owner: &Graph,
    node: &Node,
    target: &str,
    by_name: &BTreeMap<&str, &Graph>,
    issues: &mut Vec<ControlIssue>,
) {
    let Some(body) = by_name.get(target).copied() else {
        issues.push(ControlIssue::UnknownGraph {
            owner: owner.name.clone(),
            node: node.id,
            target: target.into(),
        });
        return;
    };
    let element = |ty: &SemanticType| match ty {
        SemanticType::Bytes => Some(SemanticType::Integer(crate::gir::IntegerType {
            min: 0,
            max: 255,
        })),
        SemanticType::Array(t, _) | SemanticType::Vector(t, _) | SemanticType::Slice(t) => {
            Some(t.as_ref().clone())
        }
        _ => None,
    };
    let input = node.inputs.first().and_then(|p| element(&p.ty));
    if node.inputs.len() != 1
        || body.inputs.len() != 1
        || input.as_ref() != body.inputs.first().map(|p| &p.ty)
    {
        issues.push(ControlIssue::BranchInputMismatch {
            owner: owner.name.clone(),
            node: node.id,
            target: target.into(),
        });
    }
    let output = node.outputs.first().and_then(|p| match &p.ty {
        SemanticType::Slice(t) => Some(t.as_ref()),
        _ => None,
    });
    if node.outputs.len() != 1
        || body.outputs.len() != 1
        || output != body.outputs.first().map(|p| &p.ty)
    {
        issues.push(ControlIssue::BranchOutputMismatch {
            owner: owner.name.clone(),
            node: node.id,
            target: target.into(),
        });
    }
    validate_effect_summary(owner, node, &[body], issues);
}

fn validate_select(
    owner: &Graph,
    node: &Node,
    when_true: &str,
    when_false: &str,
    by_name: &BTreeMap<&str, &Graph>,
    issues: &mut Vec<ControlIssue>,
) {
    let Some(condition) = node.inputs.first() else {
        issues.push(ControlIssue::SelectMissingCondition {
            owner: owner.name.clone(),
            node: node.id,
        });
        return;
    };

    if condition.ty != SemanticType::Bool {
        issues.push(ControlIssue::SelectConditionNotBool {
            owner: owner.name.clone(),
            node: node.id,
        });
    }

    let payload = &node.inputs[1..];
    let mut branch_graphs = Vec::new();

    for target in [when_true, when_false] {
        let Some(branch) = by_name.get(target).copied() else {
            issues.push(ControlIssue::UnknownGraph {
                owner: owner.name.clone(),
                node: node.id,
                target: target.to_owned(),
            });
            continue;
        };

        validate_branch_interface(owner, node, payload, branch, issues);
        branch_graphs.push(branch);
    }

    validate_effect_summary(owner, node, &branch_graphs, issues);
}

fn validate_match(
    owner: &Graph,
    node: &Node,
    arms: &[MatchArm],
    default: &str,
    by_name: &BTreeMap<&str, &Graph>,
    issues: &mut Vec<ControlIssue>,
) {
    let Some(selector) = node.inputs.first() else {
        issues.push(ControlIssue::MatchMissingSelector {
            owner: owner.name.clone(),
            node: node.id,
        });
        return;
    };

    if !matches!(selector.ty, SemanticType::Variant(_)) {
        issues.push(ControlIssue::MatchSelectorNotVariant {
            owner: owner.name.clone(),
            node: node.id,
        });
    }

    let mut tags = BTreeSet::new();
    for arm in arms {
        if arm.tag.is_empty() {
            issues.push(ControlIssue::EmptyMatchTag {
                owner: owner.name.clone(),
                node: node.id,
            });
        } else if !tags.insert(arm.tag.as_str()) {
            issues.push(ControlIssue::DuplicateMatchTag {
                owner: owner.name.clone(),
                node: node.id,
                tag: arm.tag.clone(),
            });
        }
    }

    let payload = &node.inputs[1..];
    let mut branch_graphs = Vec::new();

    for target in arms
        .iter()
        .map(|arm| arm.graph.as_str())
        .chain(std::iter::once(default))
    {
        let Some(branch) = by_name.get(target).copied() else {
            issues.push(ControlIssue::UnknownGraph {
                owner: owner.name.clone(),
                node: node.id,
                target: target.to_owned(),
            });
            continue;
        };

        validate_branch_interface(owner, node, payload, branch, issues);
        branch_graphs.push(branch);
    }

    validate_effect_summary(owner, node, &branch_graphs, issues);
}

fn validate_loop(
    owner: &Graph,
    node: &Node,
    condition_name: &str,
    body_name: &str,
    max_iterations: u64,
    by_name: &BTreeMap<&str, &Graph>,
    issues: &mut Vec<ControlIssue>,
) {
    if max_iterations == 0 {
        issues.push(ControlIssue::ZeroLoopBound {
            owner: owner.name.clone(),
            node: node.id,
        });
    }

    if !same_interface(&node.inputs, &node.outputs) {
        issues.push(ControlIssue::LoopStateMismatch {
            owner: owner.name.clone(),
            node: node.id,
        });
    }

    let Some(condition) = by_name.get(condition_name).copied() else {
        issues.push(ControlIssue::UnknownGraph {
            owner: owner.name.clone(),
            node: node.id,
            target: condition_name.to_owned(),
        });
        return;
    };
    let Some(body) = by_name.get(body_name).copied() else {
        issues.push(ControlIssue::UnknownGraph {
            owner: owner.name.clone(),
            node: node.id,
            target: body_name.to_owned(),
        });
        return;
    };

    if !same_interface(&node.inputs, &condition.inputs) {
        issues.push(ControlIssue::BranchInputMismatch {
            owner: owner.name.clone(),
            node: node.id,
            target: condition.name.clone(),
        });
    }

    if condition.outputs.len() != 1 || condition.outputs[0].ty != SemanticType::Bool {
        issues.push(ControlIssue::LoopConditionOutputInvalid {
            owner: owner.name.clone(),
            node: node.id,
            condition: condition.name.clone(),
        });
    }

    let (condition_effects, condition_caps) = graph_effect_summary(condition);
    if !condition_effects.is_empty() || !condition_caps.is_empty() {
        issues.push(ControlIssue::LoopConditionHasEffects {
            owner: owner.name.clone(),
            node: node.id,
            condition: condition.name.clone(),
        });
    }

    if !same_interface(&node.inputs, &body.inputs) || !same_interface(&node.outputs, &body.outputs)
    {
        issues.push(ControlIssue::LoopStateMismatch {
            owner: owner.name.clone(),
            node: node.id,
        });
    }

    validate_effect_summary(owner, node, &[body], issues);
}

fn validate_branch_interface(
    owner: &Graph,
    node: &Node,
    payload: &[Port],
    branch: &Graph,
    issues: &mut Vec<ControlIssue>,
) {
    if !same_interface(payload, &branch.inputs) {
        issues.push(ControlIssue::BranchInputMismatch {
            owner: owner.name.clone(),
            node: node.id,
            target: branch.name.clone(),
        });
    }
    if !same_interface(&node.outputs, &branch.outputs) {
        issues.push(ControlIssue::BranchOutputMismatch {
            owner: owner.name.clone(),
            node: node.id,
            target: branch.name.clone(),
        });
    }
}

fn validate_effect_summary(
    owner: &Graph,
    node: &Node,
    branches: &[&Graph],
    issues: &mut Vec<ControlIssue>,
) {
    let effects: BTreeSet<Effect> = branches
        .iter()
        .flat_map(|graph| graph_effect_summary(graph).0)
        .collect();
    let capabilities: BTreeSet<Capability> = branches
        .iter()
        .flat_map(|graph| graph_effect_summary(graph).1)
        .collect();

    if node.effects != effects || node.required_capabilities != capabilities {
        issues.push(ControlIssue::EffectSummaryMismatch {
            owner: owner.name.clone(),
            node: node.id,
        });
    }
}

fn graph_effect_summary(graph: &Graph) -> (BTreeSet<Effect>, BTreeSet<Capability>) {
    (
        graph
            .nodes
            .iter()
            .flat_map(|node| node.effects.iter().copied())
            .collect(),
        graph
            .nodes
            .iter()
            .flat_map(|node| node.required_capabilities.iter().cloned())
            .collect(),
    )
}

fn same_interface(a: &[Port], b: &[Port]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b.iter())
            .all(|(left, right)| left.name == right.name && left.ty == right.ty)
}

fn find_cycle(
    references: &BTreeMap<String, BTreeSet<String>>,
    by_name: &BTreeMap<&str, &Graph>,
) -> Option<Vec<String>> {
    fn visit(
        name: &str,
        references: &BTreeMap<String, BTreeSet<String>>,
        by_name: &BTreeMap<&str, &Graph>,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
        path: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        if visiting.contains(name) {
            let start = path.iter().position(|item| item == name).unwrap_or(0);
            let mut cycle = path[start..].to_vec();
            cycle.push(name.to_owned());
            return Some(cycle);
        }
        if visited.contains(name) {
            return None;
        }

        visiting.insert(name.to_owned());
        path.push(name.to_owned());

        if let Some(targets) = references.get(name) {
            for target in targets {
                if by_name.contains_key(target.as_str())
                    && let Some(cycle) = visit(target, references, by_name, visiting, visited, path)
                {
                    return Some(cycle);
                }
            }
        }

        path.pop();
        visiting.remove(name);
        visited.insert(name.to_owned());
        None
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut path = Vec::new();

    for name in by_name.keys() {
        if let Some(cycle) = visit(
            name,
            references,
            by_name,
            &mut visiting,
            &mut visited,
            &mut path,
        ) {
            return Some(cycle);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_requires_bool_condition_and_matching_branches() {
        let mut yes = Graph::new("yes");
        yes.inputs.push(Port {
            id: 0,
            name: "x".into(),
            ty: SemanticType::Bool,
        });
        yes.outputs.push(Port {
            id: 0,
            name: "y".into(),
            ty: SemanticType::Bool,
        });
        yes.edges.push(crate::gir::Edge {
            from: crate::gir::SourceEndpoint::GraphInput(0),
            to: crate::gir::TargetEndpoint::GraphOutput(0),
        });

        let mut no = yes.clone();
        no.name = "no".into();

        let mut main = Graph::new("main");
        main.nodes.push(Node {
            id: 1,
            operation: Operation::Select {
                when_true: "yes".into(),
                when_false: "no".into(),
            },
            inputs: vec![
                Port {
                    id: 0,
                    name: "condition".into(),
                    ty: SemanticType::Bool,
                },
                Port {
                    id: 1,
                    name: "x".into(),
                    ty: SemanticType::Bool,
                },
            ],
            outputs: vec![Port {
                id: 0,
                name: "y".into(),
                ty: SemanticType::Bool,
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        });

        assert!(validate_control_graphs(&[main, yes, no]).is_ok());
    }

    #[test]
    fn loop_requires_finite_positive_bound() {
        let state = vec![Port {
            id: 0,
            name: "x".into(),
            ty: SemanticType::Bool,
        }];

        let mut condition = Graph::new("condition");
        condition.inputs = state.clone();
        condition.outputs = vec![Port {
            id: 0,
            name: "continue".into(),
            ty: SemanticType::Bool,
        }];

        let mut body = Graph::new("body");
        body.inputs = state.clone();
        body.outputs = state.clone();

        let mut main = Graph::new("main");
        main.nodes.push(Node {
            id: 1,
            operation: Operation::Loop {
                condition: "condition".into(),
                body: "body".into(),
                max_iterations: 0,
            },
            inputs: state.clone(),
            outputs: state,
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        });

        assert!(matches!(
            validate_control_graphs(&[main, condition, body]),
            Err(issues) if issues.iter().any(|issue| matches!(
                issue,
                ControlIssue::ZeroLoopBound { .. }
            ))
        ));
    }
}
