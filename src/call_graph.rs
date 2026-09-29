use std::collections::{BTreeMap, BTreeSet};

use crate::abi::{lower_signature, AbiIssue};
use crate::gir::{Graph, Operation, SemanticType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallGraph {
    pub calls: BTreeMap<String, BTreeSet<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallGraphIssue {
    DuplicateGraph(String),
    UnknownCallee {
        caller: String,
        callee: String,
    },
    RecursiveCycle(Vec<String>),
    InputSignatureMismatch {
        caller: String,
        callee: String,
    },
    OutputSignatureMismatch {
        caller: String,
        callee: String,
    },
    AbiUnsupported {
        callee: String,
        issues: Vec<AbiIssue>,
    },
}

pub fn build_call_graph(
    graphs: &[Graph],
    external_subgraphs: &BTreeSet<String>,
) -> Result<CallGraph, Vec<CallGraphIssue>> {
    let mut issues = Vec::new();
    let mut local = BTreeSet::new();

    for graph in graphs {
        if !local.insert(graph.name.clone()) {
            issues.push(CallGraphIssue::DuplicateGraph(graph.name.clone()));
        }
    }

    let by_name: BTreeMap<&str, &Graph> =
        graphs.iter().map(|graph| (graph.name.as_str(), graph)).collect();

    let mut calls = BTreeMap::new();
    for graph in graphs {
        let mut callees = BTreeSet::new();
        for node in &graph.nodes {
            let referenced = referenced_graphs(&node.operation);
            for callee in referenced {
                if !local.contains(&callee)
                    && !external_subgraphs.contains(&callee)
                {
                    issues.push(CallGraphIssue::UnknownCallee {
                        caller: graph.name.clone(),
                        callee: callee.clone(),
                    });
                }

                if matches!(node.operation, Operation::Subgraph(_)) {
                    let node_inputs: Vec<SemanticType> =
                        node.inputs.iter().map(|port| port.ty.clone()).collect();
                    let node_outputs: Vec<SemanticType> =
                        node.outputs.iter().map(|port| port.ty.clone()).collect();

                    if let Some(target) = by_name.get(callee.as_str()).copied() {
                        let target_inputs: Vec<SemanticType> =
                            target.inputs.iter().map(|port| port.ty.clone()).collect();
                        if node_inputs != target_inputs {
                            issues.push(CallGraphIssue::InputSignatureMismatch {
                                caller: graph.name.clone(),
                                callee: callee.clone(),
                            });
                        }

                        let target_outputs: Vec<SemanticType> =
                            target.outputs.iter().map(|port| port.ty.clone()).collect();
                        if node_outputs != target_outputs {
                            issues.push(CallGraphIssue::OutputSignatureMismatch {
                                caller: graph.name.clone(),
                                callee: callee.clone(),
                            });
                        }
                    } else if external_subgraphs.contains(&callee)
                        && let Err(abi_issues) =
                            lower_signature(&node_inputs, &node_outputs)
                    {
                        issues.push(CallGraphIssue::AbiUnsupported {
                            callee: callee.clone(),
                            issues: abi_issues,
                        });
                    }
                }

                callees.insert(callee);
            }
        }
        calls.insert(graph.name.clone(), callees);
    }

    if issues.is_empty() {
        let graph = CallGraph { calls };
        if let Some(cycle) = find_local_cycle(&graph, &local) {
            return Err(vec![CallGraphIssue::RecursiveCycle(cycle)]);
        }
        Ok(graph)
    } else {
        Err(issues)
    }
}

fn referenced_graphs(operation: &Operation) -> Vec<String> {
    match operation {
        Operation::Subgraph(name) => vec![name.clone()],
        Operation::Select {
            when_true,
            when_false,
        } => vec![when_true.clone(), when_false.clone()],
        Operation::Match { arms, default } => {
            let mut result: Vec<String> =
                arms.iter().map(|arm| arm.graph.clone()).collect();
            result.push(default.clone());
            result
        }
        Operation::Loop {
            condition,
            body,
            ..
        } => vec![condition.clone(), body.clone()],
        _ => Vec::new(),
    }
}

pub fn reachable_from(
    call_graph: &CallGraph,
    entry: &str,
) -> BTreeSet<String> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![entry.to_owned()];

    while let Some(current) = stack.pop() {
        if !seen.insert(current.clone()) {
            continue;
        }
        if let Some(callees) = call_graph.calls.get(&current) {
            for callee in callees {
                if call_graph.calls.contains_key(callee) {
                    stack.push(callee.clone());
                }
            }
        }
    }

    seen
}

pub fn reachable_program_graphs(
    call_graph: &CallGraph,
    graphs: &[Graph],
    entry: &str,
) -> BTreeSet<String> {
    let by_name: BTreeMap<&str, &Graph> =
        graphs.iter().map(|graph| (graph.name.as_str(), graph)).collect();
    let mut seen = BTreeSet::new();
    let mut stack = vec![entry.to_owned()];

    while let Some(current) = stack.pop() {
        if !seen.insert(current.clone()) {
            continue;
        }

        if let Some(callees) = call_graph.calls.get(&current) {
            for callee in callees {
                if by_name.contains_key(callee.as_str()) {
                    stack.push(callee.clone());
                }
            }
        }

        if let Some(graph) = by_name.get(current.as_str()).copied() {
            for control in crate::control::control_references(graph) {
                if by_name.contains_key(control.as_str()) {
                    stack.push(control);
                }
            }
        }
    }

    seen
}

fn find_local_cycle(
    graph: &CallGraph,
    local: &BTreeSet<String>,
) -> Option<Vec<String>> {
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut path = Vec::new();

    for name in local {
        if let Some(cycle) = visit(
            name,
            graph,
            local,
            &mut visiting,
            &mut visited,
            &mut path,
        ) {
            return Some(cycle);
        }
    }

    None
}

fn visit(
    name: &str,
    graph: &CallGraph,
    local: &BTreeSet<String>,
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

    if let Some(callees) = graph.calls.get(name) {
        for callee in callees {
            if local.contains(callee)
                && let Some(cycle) = visit(
                    callee,
                    graph,
                    local,
                    visiting,
                    visited,
                    path,
                )
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{Node, Operation};

    fn graph(name: &str, callee: Option<&str>) -> Graph {
        let mut graph = Graph::new(name);
        if let Some(callee) = callee {
            graph.nodes.push(Node {
                id: 1,
                operation: Operation::Subgraph(callee.into()),
                inputs: vec![],
                outputs: vec![],
                effects: BTreeSet::new(),
                required_capabilities: BTreeSet::new(),
            });
        }
        graph
    }

    #[test]
    fn local_call_resolves_and_reachability_is_computed() {
        let call_graph = build_call_graph(
            &[graph("main", Some("worker")), graph("worker", None)],
            &BTreeSet::new(),
        )
        .unwrap();

        assert_eq!(
            reachable_from(&call_graph, "main"),
            BTreeSet::from(["main".into(), "worker".into()])
        );
    }

    #[test]
    fn local_call_signature_must_match_callee() {
        use crate::gir::{IntegerType, Port, SemanticType};

        let mut main = graph("main", None);
        main.nodes.push(Node {
            id: 1,
            operation: Operation::Subgraph("worker".into()),
            inputs: vec![Port {
                id: 0,
                name: "x".into(),
                ty: SemanticType::Bool,
            }],
            outputs: vec![],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        });

        let mut worker = graph("worker", None);
        worker.inputs.push(Port {
            id: 0,
            name: "x".into(),
            ty: SemanticType::Integer(
                IntegerType::new(0, 10).unwrap(),
            ),
        });

        assert!(matches!(
            build_call_graph(&[main, worker], &BTreeSet::new()),
            Err(issues) if issues.iter().any(|issue| matches!(
                issue,
                CallGraphIssue::InputSignatureMismatch { .. }
            ))
        ));
    }

    #[test]
    fn structured_control_graphs_are_reachable() {
        let mut main = graph("main", None);
        main.nodes.push(Node {
            id: 1,
            operation: Operation::Select {
                when_true: "yes".into(),
                when_false: "no".into(),
            },
            inputs: vec![],
            outputs: vec![],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        });

        let call_graph = build_call_graph(
            &[main, graph("yes", None), graph("no", None)],
            &BTreeSet::new(),
        )
        .unwrap();

        assert_eq!(
            reachable_from(&call_graph, "main"),
            BTreeSet::from([
                "main".into(),
                "no".into(),
                "yes".into(),
            ])
        );
    }

    #[test]
    fn unknown_call_is_rejected() {
        assert!(matches!(
            build_call_graph(
                &[graph("main", Some("missing"))],
                &BTreeSet::new(),
            ),
            Err(issues) if matches!(
                issues.first(),
                Some(CallGraphIssue::UnknownCallee { .. })
            )
        ));
    }

    #[test]
    fn raw_recursive_cycle_is_rejected() {
        assert!(matches!(
            build_call_graph(
                &[
                    graph("a", Some("b")),
                    graph("b", Some("a")),
                ],
                &BTreeSet::new(),
            ),
            Err(issues) if matches!(
                issues.first(),
                Some(CallGraphIssue::RecursiveCycle(_))
            )
        ));
    }
}
