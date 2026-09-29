use std::collections::{BTreeMap, BTreeSet};

use crate::call_graph::build_call_graph;
use crate::gir::{
    Edge, Graph, Node, NodeId, Operation, Port, SourceEndpoint, TargetEndpoint,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineOrigin {
    pub graph: String,
    pub original_node: NodeId,
    pub call_stack: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlinedGraph {
    pub graph: Graph,
    pub origins: BTreeMap<NodeId, InlineOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineIssue {
    UnknownEntry(String),
    InvalidCallGraph,
    ExternalCallNotInlineable {
        caller: String,
        callee: String,
    },
    EffectSummaryMismatch {
        call_node: NodeId,
        callee: String,
    },
    InterfaceMismatch {
        caller: String,
        call_node: NodeId,
        callee: String,
    },
    MissingCallInput {
        call_node: NodeId,
        port: String,
    },
    MissingCalleeOutput {
        callee: String,
        port: String,
    },
    NodeIdExhausted,
    InvalidResult,
}

pub fn inline_entry(
    graphs: &[Graph],
    entry: &str,
    external_subgraphs: &BTreeSet<String>,
) -> Result<InlinedGraph, Vec<InlineIssue>> {
    if build_call_graph(graphs, external_subgraphs).is_err() {
        return Err(vec![InlineIssue::InvalidCallGraph]);
    }

    let by_name: BTreeMap<&str, &Graph> =
        graphs.iter().map(|graph| (graph.name.as_str(), graph)).collect();
    let Some(entry_graph) = by_name.get(entry).copied() else {
        return Err(vec![InlineIssue::UnknownEntry(entry.to_owned())]);
    };

    let mut result = InlinedGraph {
        graph: entry_graph.clone(),
        origins: entry_graph
            .nodes
            .iter()
            .map(|node| {
                (
                    node.id,
                    InlineOrigin {
                        graph: entry_graph.name.clone(),
                        original_node: node.id,
                        call_stack: vec![entry_graph.name.clone()],
                    },
                )
            })
            .collect(),
    };

    loop {
        let call = result
            .graph
            .nodes
            .iter()
            .filter_map(|node| match &node.operation {
                Operation::Subgraph(callee)
                    if !external_subgraphs.contains(callee) =>
                {
                    Some((node.id, callee.clone()))
                }
                _ => None,
            })
            .min_by_key(|(id, _)| *id);

        let Some((call_id, callee_name)) = call else {
            break;
        };

        let Some(callee) = by_name.get(callee_name.as_str()).copied() else {
            return Err(vec![InlineIssue::ExternalCallNotInlineable {
                caller: result.graph.name.clone(),
                callee: callee_name,
            }]);
        };

        inline_one(&mut result, call_id, callee)?;
    }

    if crate::gir_validate::validate(&result.graph).is_err() {
        return Err(vec![InlineIssue::InvalidResult]);
    }

    Ok(result)
}

fn inline_one(
    result: &mut InlinedGraph,
    call_id: NodeId,
    callee: &Graph,
) -> Result<(), Vec<InlineIssue>> {
    let Some(call_node) = result
        .graph
        .nodes
        .iter()
        .find(|node| node.id == call_id)
        .cloned()
    else {
        return Err(vec![InlineIssue::InvalidResult]);
    };

    let callee_effects: BTreeSet<_> = callee
        .nodes
        .iter()
        .flat_map(|node| node.effects.iter().copied())
        .collect();
    let callee_capabilities: BTreeSet<_> = callee
        .nodes
        .iter()
        .flat_map(|node| node.required_capabilities.iter().cloned())
        .collect();

    if call_node.effects != callee_effects
        || call_node.required_capabilities != callee_capabilities
    {
        return Err(vec![InlineIssue::EffectSummaryMismatch {
            call_node: call_id,
            callee: callee.name.clone(),
        }]);
    }

    if !interfaces_match(&call_node.inputs, &callee.inputs)
        || !interfaces_match(&call_node.outputs, &callee.outputs)
    {
        return Err(vec![InlineIssue::InterfaceMismatch {
            caller: result.graph.name.clone(),
            call_node: call_id,
            callee: callee.name.clone(),
        }]);
    }

    let call_origin = result.origins.get(&call_id).cloned();
    let mut call_stack = call_origin
        .map(|origin| origin.call_stack)
        .unwrap_or_else(|| vec![result.graph.name.clone()]);
    call_stack.push(callee.name.clone());

    let incoming = input_sources(&result.graph, &call_node)?;
    let incoming_by_callee_port: BTreeMap<u16, SourceEndpoint> = callee
        .inputs
        .iter()
        .filter_map(|port| {
            incoming
                .get(&port.name)
                .cloned()
                .map(|source| (port.id, source))
        })
        .collect();
    let outgoing = output_targets(&result.graph, &call_node);

    let mut next_id = result
        .graph
        .nodes
        .iter()
        .map(|node| node.id)
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or_else(|| vec![InlineIssue::NodeIdExhausted])?;

    let mut id_map = BTreeMap::<NodeId, NodeId>::new();
    for node in &callee.nodes {
        id_map.insert(node.id, next_id);
        next_id = next_id
            .checked_add(1)
            .ok_or_else(|| vec![InlineIssue::NodeIdExhausted])?;
    }

    let mut new_nodes = Vec::new();
    for node in &callee.nodes {
        let new_id = id_map[&node.id];
        let mut clone = node.clone();
        clone.id = new_id;
        new_nodes.push(clone);
        result.origins.insert(
            new_id,
            InlineOrigin {
                graph: callee.name.clone(),
                original_node: node.id,
                call_stack: call_stack.clone(),
            },
        );
    }

    let mut new_edges = Vec::new();
    for edge in &callee.edges {
        if matches!(edge.to, TargetEndpoint::GraphOutput(_)) {
            continue;
        }

        let from = map_source(
            &edge.from,
            &id_map,
            &incoming_by_callee_port,
        )?;
        let to = match edge.to {
            TargetEndpoint::NodeInput { node, port } => {
                TargetEndpoint::NodeInput {
                    node: id_map[&node],
                    port,
                }
            }
            TargetEndpoint::GraphOutput(_) => unreachable!(),
        };
        new_edges.push(Edge { from, to });
    }

    for callee_output in &callee.outputs {
        let source = resolve_callee_output(
            callee,
            callee_output,
            &id_map,
            &incoming,
        )?;

        if let Some(call_port) = call_node
            .outputs
            .iter()
            .find(|port| port.name == callee_output.name)
        {
            if let Some(targets) = outgoing.get(&call_port.id) {
                for target in targets {
                    new_edges.push(Edge {
                        from: source.clone(),
                        to: target.clone(),
                    });
                }
            }
        }
    }

    result.graph.nodes.retain(|node| node.id != call_id);
    result.graph.edges.retain(|edge| {
        !matches!(
            edge.from,
            SourceEndpoint::NodeOutput { node, .. } if node == call_id
        ) && !matches!(
            edge.to,
            TargetEndpoint::NodeInput { node, .. } if node == call_id
        )
    });
    result.origins.remove(&call_id);

    result.graph.nodes.extend(new_nodes);
    result.graph.edges.extend(new_edges);

    Ok(())
}

fn interfaces_match(a: &[Port], b: &[Port]) -> bool {
    if a.len() != b.len() {
        return false;
    }

    a.iter().all(|port| {
        b.iter()
            .find(|candidate| candidate.name == port.name)
            .is_some_and(|candidate| candidate.ty == port.ty)
    })
}

fn input_sources(
    graph: &Graph,
    call: &Node,
) -> Result<BTreeMap<String, SourceEndpoint>, Vec<InlineIssue>> {
    let mut sources = BTreeMap::new();

    for port in &call.inputs {
        let source = graph.edges.iter().find_map(|edge| {
            match &edge.to {
                TargetEndpoint::NodeInput { node, port: target_port }
                    if *node == call.id && *target_port == port.id =>
                {
                    Some(edge.from.clone())
                }
                _ => None,
            }
        });

        let Some(source) = source else {
            return Err(vec![InlineIssue::MissingCallInput {
                call_node: call.id,
                port: port.name.clone(),
            }]);
        };
        sources.insert(port.name.clone(), source);
    }

    Ok(sources)
}

fn output_targets(
    graph: &Graph,
    call: &Node,
) -> BTreeMap<u16, Vec<TargetEndpoint>> {
    let mut targets = BTreeMap::<u16, Vec<TargetEndpoint>>::new();

    for edge in &graph.edges {
        if let SourceEndpoint::NodeOutput { node, port } = edge.from
            && node == call.id
        {
            targets.entry(port).or_default().push(edge.to.clone());
        }
    }

    targets
}

fn map_source(
    source: &SourceEndpoint,
    ids: &BTreeMap<NodeId, NodeId>,
    incoming: &BTreeMap<u16, SourceEndpoint>,
) -> Result<SourceEndpoint, Vec<InlineIssue>> {
    match source {
        SourceEndpoint::NodeOutput { node, port } => {
            Ok(SourceEndpoint::NodeOutput {
                node: ids[node],
                port: *port,
            })
        }
        SourceEndpoint::GraphInput(port) => incoming
            .get(port)
            .cloned()
            .ok_or_else(|| {
                vec![InlineIssue::MissingCalleeOutput {
                    callee: "callee-input".into(),
                    port: port.to_string(),
                }]
            }),
    }
}

fn resolve_callee_output(
    callee: &Graph,
    output: &Port,
    ids: &BTreeMap<NodeId, NodeId>,
    incoming: &BTreeMap<String, SourceEndpoint>,
) -> Result<SourceEndpoint, Vec<InlineIssue>> {
    let edge = callee.edges.iter().find(|edge| {
        matches!(
            edge.to,
            TargetEndpoint::GraphOutput(port) if port == output.id
        )
    });

    let Some(edge) = edge else {
        return Err(vec![InlineIssue::MissingCalleeOutput {
            callee: callee.name.clone(),
            port: output.name.clone(),
        }]);
    };

    match &edge.from {
        SourceEndpoint::NodeOutput { node, port } => {
            Ok(SourceEndpoint::NodeOutput {
                node: ids[node],
                port: *port,
            })
        }
        SourceEndpoint::GraphInput(port_id) => {
            let Some(input) =
                callee.inputs.iter().find(|port| port.id == *port_id)
            else {
                return Err(vec![InlineIssue::MissingCalleeOutput {
                    callee: callee.name.clone(),
                    port: output.name.clone(),
                }]);
            };

            incoming.get(&input.name).cloned().ok_or_else(|| {
                vec![InlineIssue::MissingCallInput {
                    call_node: 0,
                    port: input.name.clone(),
                }]
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{AuthorityMode, IntegerType, Literal, SemanticType};

    fn int() -> SemanticType {
        SemanticType::Integer(IntegerType::new(0, 100).unwrap())
    }

    fn int_plus_one() -> SemanticType {
        SemanticType::Integer(IntegerType::new(1, 101).unwrap())
    }

    #[test]
    fn local_subgraph_is_inlined_and_traceable() {
        let worker = Graph {
            name: "worker".into(),
            inputs: vec![Port {
                id: 0,
                name: "x".into(),
                ty: int(),
            }],
            outputs: vec![Port {
                id: 0,
                name: "y".into(),
                ty: int_plus_one(),
            }],
            nodes: vec![
                Node {
                    id: 1,
                    operation: Operation::Const(Literal::Integer(1)),
                    inputs: vec![],
                    outputs: vec![Port {
                        id: 0,
                        name: "one".into(),
                        ty: SemanticType::Integer(
                            IntegerType::new(1, 1).unwrap(),
                        ),
                    }],
                    effects: BTreeSet::new(),
                    required_capabilities: BTreeSet::new(),
                },
                Node {
                    id: 2,
                    operation: Operation::Add,
                    inputs: vec![
                        Port {
                            id: 0,
                            name: "x".into(),
                            ty: int(),
                        },
                        Port {
                            id: 1,
                            name: "one".into(),
                            ty: SemanticType::Integer(
                                IntegerType::new(1, 1).unwrap(),
                            ),
                        },
                    ],
                    outputs: vec![Port {
                        id: 0,
                        name: "y".into(),
                        ty: int_plus_one(),
                    }],
                    effects: BTreeSet::new(),
                    required_capabilities: BTreeSet::new(),
                },
            ],
            edges: vec![
                Edge {
                    from: SourceEndpoint::GraphInput(0),
                    to: TargetEndpoint::NodeInput { node: 2, port: 0 },
                },
                Edge {
                    from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
                    to: TargetEndpoint::NodeInput { node: 2, port: 1 },
                },
                Edge {
                    from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
                    to: TargetEndpoint::GraphOutput(0),
                },
            ],
            authority: AuthorityMode::DefaultDeny,
        };

        let main = Graph {
            name: "main".into(),
            inputs: vec![Port {
                id: 0,
                name: "x".into(),
                ty: int(),
            }],
            outputs: vec![Port {
                id: 0,
                name: "y".into(),
                ty: int_plus_one(),
            }],
            nodes: vec![Node {
                id: 10,
                operation: Operation::Subgraph("worker".into()),
                inputs: vec![Port {
                    id: 0,
                    name: "x".into(),
                    ty: int(),
                }],
                outputs: vec![Port {
                    id: 0,
                    name: "y".into(),
                    ty: int_plus_one(),
                }],
                effects: BTreeSet::new(),
                required_capabilities: BTreeSet::new(),
            }],
            edges: vec![
                Edge {
                    from: SourceEndpoint::GraphInput(0),
                    to: TargetEndpoint::NodeInput { node: 10, port: 0 },
                },
                Edge {
                    from: SourceEndpoint::NodeOutput { node: 10, port: 0 },
                    to: TargetEndpoint::GraphOutput(0),
                },
            ],
            authority: AuthorityMode::DefaultDeny,
        };

        let result =
            inline_entry(&[main, worker], "main", &BTreeSet::new()).unwrap();

        assert!(result
            .graph
            .nodes
            .iter()
            .all(|node| !matches!(node.operation, Operation::Subgraph(_))));
        assert!(result
            .origins
            .values()
            .any(|origin| origin.graph == "worker"));
    }
}
