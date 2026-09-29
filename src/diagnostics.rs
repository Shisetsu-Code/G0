use crate::gir::NodeId;
use crate::profiling::{
    summarize_debug, CostMetric, CriticalPathIssue, DebugSummary, Hotspot,
    ProfileReport,
};
use crate::gir::Graph;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottleneckKind {
    Cpu,
    Latency,
    Queue,
    Blocking,
    IoWait,
    MemoryAllocation,
    Network,
    Storage,
    CriticalPath,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PerformanceDiagnostic {
    pub kind: BottleneckKind,
    pub graph: String,
    pub node: Option<NodeId>,
    pub contribution_ppm: Option<u32>,
    pub value: u128,
    pub message: String,
}

pub fn diagnose_performance(
    graph: &Graph,
    report: &ProfileReport,
    threshold_ppm: u32,
    top_n: usize,
) -> Result<Vec<PerformanceDiagnostic>, CriticalPathIssue> {
    let summary = summarize_debug(graph, report, top_n)?;
    let mut diagnostics = Vec::new();

    append_hotspots(
        &mut diagnostics,
        BottleneckKind::Cpu,
        "CPU",
        &summary.cpu_hotspots,
        threshold_ppm,
    );
    append_hotspots(
        &mut diagnostics,
        BottleneckKind::Latency,
        "p99 latency",
        &summary.latency_hotspots,
        threshold_ppm,
    );
    append_hotspots(
        &mut diagnostics,
        BottleneckKind::Queue,
        "scheduler/queue time",
        &summary.queue_hotspots,
        threshold_ppm,
    );
    append_hotspots(
        &mut diagnostics,
        BottleneckKind::Blocking,
        "blocked time",
        &summary.blocked_hotspots,
        threshold_ppm,
    );
    append_hotspots(
        &mut diagnostics,
        BottleneckKind::IoWait,
        "I/O wait",
        &summary.io_wait_hotspots,
        threshold_ppm,
    );
    append_hotspots(
        &mut diagnostics,
        BottleneckKind::MemoryAllocation,
        "allocated bytes",
        &summary.memory_hotspots,
        threshold_ppm,
    );
    append_hotspots(
        &mut diagnostics,
        BottleneckKind::Network,
        "network traffic",
        &summary.network_hotspots,
        threshold_ppm,
    );
    append_hotspots(
        &mut diagnostics,
        BottleneckKind::Storage,
        "storage traffic",
        &summary.storage_hotspots,
        threshold_ppm,
    );

    if !summary.critical_path.nodes.is_empty() {
        diagnostics.push(PerformanceDiagnostic {
            kind: BottleneckKind::CriticalPath,
            graph: graph.name.clone(),
            node: summary.critical_path.nodes.last().copied(),
            contribution_ppm: None,
            value: summary.critical_path.p99_ns,
            message: format!(
                "critical p99 path {:?} totals {} ns",
                summary.critical_path.nodes, summary.critical_path.p99_ns
            ),
        });
    }

    Ok(diagnostics)
}

fn append_hotspots(
    diagnostics: &mut Vec<PerformanceDiagnostic>,
    kind: BottleneckKind,
    label: &str,
    hotspots: &[Hotspot],
    threshold_ppm: u32,
) {
    for hotspot in hotspots {
        if hotspot.share_ppm < threshold_ppm {
            continue;
        }

        diagnostics.push(PerformanceDiagnostic {
            kind,
            graph: hotspot.location.graph.clone(),
            node: Some(hotspot.location.node),
            contribution_ppm: Some(hotspot.share_ppm),
            value: hotspot.value,
            message: format!(
                "node {} '{}' contributes {:.2}% of {}",
                hotspot.location.node,
                hotspot.location.label,
                hotspot.share_ppm as f64 / 10_000.0,
                label
            ),
        });
    }
}

pub fn metric_name(metric: CostMetric) -> &'static str {
    match metric {
        CostMetric::WallTime => "wall_ns",
        CostMetric::CpuTime => "cpu_ns",
        CostMetric::QueueTime => "queue_ns",
        CostMetric::BlockedTime => "blocked_ns",
        CostMetric::IoWaitTime => "io_wait_ns",
        CostMetric::AllocatedBytes => "allocated_bytes",
        CostMetric::PeakLiveBytes => "peak_live_bytes",
        CostMetric::NetworkBytes => "network_bytes",
        CostMetric::StorageBytes => "storage_bytes",
        CostMetric::P99Latency => "p99_ns",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeightReport {
    pub cpu: Vec<(NodeId, u32)>,
    pub latency: Vec<(NodeId, u32)>,
    pub memory: Vec<(NodeId, u32)>,
    pub network: Vec<(NodeId, u32)>,
    pub storage: Vec<(NodeId, u32)>,
}

pub fn graph_weights(summary: &DebugSummary) -> WeightReport {
    WeightReport {
        cpu: to_weights(&summary.cpu_hotspots),
        latency: to_weights(&summary.latency_hotspots),
        memory: to_weights(&summary.memory_hotspots),
        network: to_weights(&summary.network_hotspots),
        storage: to_weights(&summary.storage_hotspots),
    }
}

fn to_weights(hotspots: &[Hotspot]) -> Vec<(NodeId, u32)> {
    hotspots
        .iter()
        .map(|hotspot| (hotspot.location.node, hotspot.share_ppm))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use super::*;
    use crate::gir::{
        AuthorityMode, Node, Operation, Port, SemanticType,
    };
    use crate::profiling::{NodeLocation, NodeMetrics, NodeProfile};

    fn node(id: NodeId) -> Node {
        Node {
            id,
            operation: Operation::Subgraph(format!("node-{id}")),
            inputs: vec![],
            outputs: vec![Port {
                id: 0,
                name: "out".into(),
                ty: SemanticType::Bytes,
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        }
    }

    #[test]
    fn strong_cpu_weight_is_reported_with_node_identity() {
        let graph = Graph {
            name: "pipeline".into(),
            inputs: vec![],
            outputs: vec![],
            nodes: vec![node(1), node(2)],
            edges: vec![],
            authority: AuthorityMode::DefaultDeny,
        };
        let report = ProfileReport {
            nodes: BTreeMap::from([
                (
                    1,
                    NodeProfile {
                        location: NodeLocation {
                            graph: "pipeline".into(),
                            node: 1,
                            label: "decode".into(),
                        },
                        metrics: NodeMetrics {
                            cpu_ns_total: 900,
                            latency_p99_ns: 100,
                            ..NodeMetrics::default()
                        },
                    },
                ),
                (
                    2,
                    NodeProfile {
                        location: NodeLocation {
                            graph: "pipeline".into(),
                            node: 2,
                            label: "store".into(),
                        },
                        metrics: NodeMetrics {
                            cpu_ns_total: 100,
                            latency_p99_ns: 900,
                            ..NodeMetrics::default()
                        },
                    },
                ),
            ]),
        };

        let diagnostics =
            diagnose_performance(&graph, &report, 500_000, 4).unwrap();

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == BottleneckKind::Cpu
                && diagnostic.node == Some(1)
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == BottleneckKind::Latency
                && diagnostic.node == Some(2)
        }));
    }
}
