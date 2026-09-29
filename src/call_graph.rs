use std::collections::{BTreeMap, BTreeSet};

use crate::gir::{Graph, Operation};

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

    let mut calls = BTreeMap::new();
    for graph in graphs {
        let mut callees = BTreeSet::new();
        for node in &graph.nodes {
            if let Operation::Subgraph(callee) = &node.operation {
                if !local.contains(callee)
                    && !external_subgraphs.contains(callee)
                {
                    issues.push(CallGraphIssue::UnknownCallee {
                        caller: graph.name.clone(),
                        callee: callee.clone(),
                    });
                }
                callees.insert(callee.clone());
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
