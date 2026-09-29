use std::collections::{BTreeMap, BTreeSet};

use crate::gir::{Graph, NodeId, SemanticType, SourceEndpoint, TargetEndpoint};
use crate::information_flow::{classify_semantic_type, DataClass};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeLocation {
    pub graph: String,
    pub node: NodeId,
    pub label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NodeMetrics {
    pub calls: u64,
    pub wall_ns_total: u128,
    pub cpu_ns_total: u128,
    pub latency_p50_ns: u64,
    pub latency_p95_ns: u64,
    pub latency_p99_ns: u64,
    pub allocations: u64,
    pub allocated_bytes: u128,
    pub peak_live_bytes: u64,
    pub bytes_read: u128,
    pub bytes_written: u128,
    pub network_bytes: u128,
    pub storage_bytes: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostMetric {
    WallTime,
    CpuTime,
    AllocatedBytes,
    PeakLiveBytes,
    NetworkBytes,
    StorageBytes,
    P99Latency,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeProfile {
    pub location: NodeLocation,
    pub metrics: NodeMetrics,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProfileReport {
    pub nodes: BTreeMap<NodeId, NodeProfile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hotspot {
    pub location: NodeLocation,
    pub value: u128,
    /// Contribution to the selected aggregate in parts per million.
    pub share_ppm: u32,
}

impl ProfileReport {
    pub fn rank_hotspots(
        &self,
        metric: CostMetric,
        limit: usize,
    ) -> Vec<Hotspot> {
        let total: u128 = self
            .nodes
            .values()
            .map(|profile| metric_value(profile.metrics, metric))
            .sum();

        let mut hotspots: Vec<Hotspot> = self
            .nodes
            .values()
            .map(|profile| {
                let value = metric_value(profile.metrics, metric);
                let share_ppm = value
                    .saturating_mul(1_000_000)
                    .checked_div(total)
                    .unwrap_or(0)
                    .min(u32::MAX as u128) as u32;
                Hotspot {
                    location: profile.location.clone(),
                    value,
                    share_ppm,
                }
            })
            .collect();

        hotspots.sort_by(|a, b| {
            b.value
                .cmp(&a.value)
                .then_with(|| a.location.node.cmp(&b.location.node))
        });
        hotspots.truncate(limit);
        hotspots
    }
}

fn metric_value(metrics: NodeMetrics, metric: CostMetric) -> u128 {
    match metric {
        CostMetric::WallTime => metrics.wall_ns_total,
        CostMetric::CpuTime => metrics.cpu_ns_total,
        CostMetric::AllocatedBytes => metrics.allocated_bytes,
        CostMetric::PeakLiveBytes => metrics.peak_live_bytes as u128,
        CostMetric::NetworkBytes => metrics.network_bytes,
        CostMetric::StorageBytes => metrics.storage_bytes,
        CostMetric::P99Latency => metrics.latency_p99_ns as u128,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriticalPath {
    pub nodes: Vec<NodeId>,
    pub p99_ns: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CriticalPathIssue {
    MissingMetrics(NodeId),
    RawCycle,
}

pub fn critical_path_p99(
    graph: &Graph,
    report: &ProfileReport,
) -> Result<CriticalPath, CriticalPathIssue> {
    let node_ids: BTreeSet<NodeId> =
        graph.nodes.iter().map(|node| node.id).collect();
    let mut indegree: BTreeMap<NodeId, usize> =
        node_ids.iter().map(|id| (*id, 0)).collect();
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

    for node in &node_ids {
        if !report.nodes.contains_key(node) {
            return Err(CriticalPathIssue::MissingMetrics(*node));
        }
    }

    let mut ready: BTreeSet<NodeId> = indegree
        .iter()
        .filter_map(|(node, degree)| (*degree == 0).then_some(*node))
        .collect();
    let mut order = Vec::with_capacity(node_ids.len());

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

    if order.len() != node_ids.len() {
        return Err(CriticalPathIssue::RawCycle);
    }

    let mut best_cost: BTreeMap<NodeId, u128> = BTreeMap::new();
    let mut predecessor: BTreeMap<NodeId, NodeId> = BTreeMap::new();

    for node in order {
        let own = report.nodes[&node].metrics.latency_p99_ns as u128;
        let mut incoming_best = 0_u128;
        let mut incoming_node = None;

        for edge in &graph.edges {
            if let (
                SourceEndpoint::NodeOutput { node: from, .. },
                TargetEndpoint::NodeInput { node: to, .. },
            ) = (&edge.from, &edge.to)
                && *to == node
            {
                let candidate = best_cost.get(from).copied().unwrap_or(0);
                if candidate > incoming_best {
                    incoming_best = candidate;
                    incoming_node = Some(*from);
                }
            }
        }

        best_cost.insert(node, incoming_best.saturating_add(own));
        if let Some(previous) = incoming_node {
            predecessor.insert(node, previous);
        }
    }

    let Some((&end, &cost)) = best_cost.iter().max_by_key(|(_, cost)| *cost)
    else {
        return Ok(CriticalPath {
            nodes: Vec::new(),
            p99_ns: 0,
        });
    };

    let mut path = vec![end];
    let mut current = end;
    while let Some(previous) = predecessor.get(&current).copied() {
        path.push(previous);
        current = previous;
    }
    path.reverse();

    Ok(CriticalPath {
        nodes: path,
        p99_ns: cost,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugValueVisibility {
    ValueAllowed,
    MetadataOnly,
    FullyRedacted,
}

pub fn debug_visibility(ty: &SemanticType) -> DebugValueVisibility {
    match classify_semantic_type(ty) {
        DataClass::Public => DebugValueVisibility::ValueAllowed,
        DataClass::Private => DebugValueVisibility::MetadataOnly,
        DataClass::Secret | DataClass::Credential => {
            DebugValueVisibility::FullyRedacted
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugSummary {
    pub cpu_hotspots: Vec<Hotspot>,
    pub latency_hotspots: Vec<Hotspot>,
    pub memory_hotspots: Vec<Hotspot>,
    pub critical_path: CriticalPath,
}

pub fn summarize_debug(
    graph: &Graph,
    report: &ProfileReport,
    top_n: usize,
) -> Result<DebugSummary, CriticalPathIssue> {
    Ok(DebugSummary {
        cpu_hotspots: report.rank_hotspots(CostMetric::CpuTime, top_n),
        latency_hotspots: report.rank_hotspots(CostMetric::P99Latency, top_n),
        memory_hotspots: report.rank_hotspots(CostMetric::AllocatedBytes, top_n),
        critical_path: critical_path_p99(graph, report)?,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{
        AuthorityMode, Edge, Node, Operation, Port, TargetEndpoint,
    };

    fn node(id: NodeId) -> Node {
        Node {
            id,
            operation: Operation::Subgraph(format!("n{id}")),
            inputs: vec![Port {
                id: 0,
                name: "in".into(),
                ty: SemanticType::Text,
            }],
            outputs: vec![Port {
                id: 0,
                name: "out".into(),
                ty: SemanticType::Text,
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        }
    }

    fn profile(id: NodeId, p99: u64, cpu: u128, allocated: u128) -> NodeProfile {
        NodeProfile {
            location: NodeLocation {
                graph: "test".into(),
                node: id,
                label: format!("node-{id}"),
            },
            metrics: NodeMetrics {
                latency_p99_ns: p99,
                cpu_ns_total: cpu,
                allocated_bytes: allocated,
                ..NodeMetrics::default()
            },
        }
    }

    #[test]
    fn ranks_heaviest_cpu_node_first() {
        let report = ProfileReport {
            nodes: BTreeMap::from([
                (1, profile(1, 10, 100, 50)),
                (2, profile(2, 20, 900, 20)),
            ]),
        };

        let hotspots = report.rank_hotspots(CostMetric::CpuTime, 2);
        assert_eq!(hotspots[0].location.node, 2);
        assert_eq!(hotspots[0].share_ppm, 900_000);
    }

    #[test]
    fn computes_graph_critical_path_from_p99_latency() {
        let graph = Graph {
            name: "test".into(),
            inputs: vec![],
            outputs: vec![],
            nodes: vec![node(1), node(2), node(3)],
            edges: vec![
                Edge {
                    from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
                    to: TargetEndpoint::NodeInput { node: 3, port: 0 },
                },
                Edge {
                    from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
                    to: TargetEndpoint::NodeInput { node: 3, port: 0 },
                },
            ],
            authority: AuthorityMode::DefaultDeny,
        };

        let report = ProfileReport {
            nodes: BTreeMap::from([
                (1, profile(1, 100, 0, 0)),
                (2, profile(2, 500, 0, 0)),
                (3, profile(3, 200, 0, 0)),
            ]),
        };

        let path = critical_path_p99(&graph, &report).unwrap();
        assert_eq!(path.nodes, vec![2, 3]);
        assert_eq!(path.p99_ns, 700);
    }

    #[test]
    fn secrets_are_never_value_visible_in_debugger() {
        let secret = SemanticType::Secret(Box::new(SemanticType::Text));
        let credential =
            SemanticType::Credential(Box::new(SemanticType::Text));

        assert_eq!(
            debug_visibility(&secret),
            DebugValueVisibility::FullyRedacted
        );
        assert_eq!(
            debug_visibility(&credential),
            DebugValueVisibility::FullyRedacted
        );
    }
}
