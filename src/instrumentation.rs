use std::collections::{BTreeMap, BTreeSet};

use crate::gir::{Effect, Graph, NodeId, SemanticType};
use crate::information_flow::{classify_semantic_type, DataClass};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InstrumentMetric {
    WallTime,
    CpuTime,
    QueueTime,
    BlockedTime,
    AllocatedBytes,
    PeakLiveBytes,
    NetworkBytes,
    StorageBytes,
    CallCount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstrumentationMode {
    Off,
    Sampled,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeInstrumentation {
    pub node: NodeId,
    pub metrics: BTreeSet<InstrumentMetric>,
    pub sample_every: u32,
    pub capture_values: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InstrumentationPlan {
    pub nodes: BTreeMap<NodeId, NodeInstrumentation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstrumentationBudget {
    pub max_overhead_ppm: u32,
    pub estimated_probe_ns: u64,
    pub expected_calls_per_s: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstrumentationIssue {
    ZeroSampleRate(NodeId),
    SensitiveValueCapture(NodeId),
    OverheadBudgetExceeded {
        estimated_ppm: u32,
        maximum_ppm: u32,
    },
}

pub fn build_instrumentation_plan(
    graph: &Graph,
    mode: InstrumentationMode,
    budget: InstrumentationBudget,
) -> Result<InstrumentationPlan, Vec<InstrumentationIssue>> {
    if mode == InstrumentationMode::Off {
        return Ok(InstrumentationPlan::default());
    }

    let sample_every = choose_sampling(mode, budget);
    let estimated_ppm = estimate_overhead_ppm(sample_every, budget);
    if estimated_ppm > budget.max_overhead_ppm {
        return Err(vec![
            InstrumentationIssue::OverheadBudgetExceeded {
                estimated_ppm,
                maximum_ppm: budget.max_overhead_ppm,
            },
        ]);
    }

    let mut nodes = BTreeMap::new();
    for node in &graph.nodes {
        let mut metrics = BTreeSet::from([
            InstrumentMetric::WallTime,
            InstrumentMetric::CpuTime,
            InstrumentMetric::CallCount,
        ]);

        if node.effects.contains(&Effect::Network) {
            metrics.insert(InstrumentMetric::NetworkBytes);
            metrics.insert(InstrumentMetric::BlockedTime);
        }
        if node.effects.contains(&Effect::Storage) {
            metrics.insert(InstrumentMetric::StorageBytes);
            metrics.insert(InstrumentMetric::BlockedTime);
        }
        if node.effects.contains(&Effect::MemoryWrite) {
            metrics.insert(InstrumentMetric::AllocatedBytes);
            metrics.insert(InstrumentMetric::PeakLiveBytes);
        }

        let sensitive = node
            .inputs
            .iter()
            .chain(node.outputs.iter())
            .any(|port| sensitive_type(&port.ty));

        nodes.insert(
            node.id,
            NodeInstrumentation {
                node: node.id,
                metrics,
                sample_every,
                capture_values: !sensitive && mode == InstrumentationMode::Full,
            },
        );
    }

    let plan = InstrumentationPlan { nodes };
    validate_instrumentation_plan(graph, &plan)?;
    Ok(plan)
}

pub fn validate_instrumentation_plan(
    graph: &Graph,
    plan: &InstrumentationPlan,
) -> Result<(), Vec<InstrumentationIssue>> {
    let mut issues = Vec::new();

    for (node_id, item) in &plan.nodes {
        if item.sample_every == 0 {
            issues.push(InstrumentationIssue::ZeroSampleRate(*node_id));
        }

        let sensitive = graph
            .nodes
            .iter()
            .find(|node| node.id == *node_id)
            .is_some_and(|node| {
                node.inputs
                    .iter()
                    .chain(node.outputs.iter())
                    .any(|port| sensitive_type(&port.ty))
            });

        if sensitive && item.capture_values {
            issues.push(InstrumentationIssue::SensitiveValueCapture(
                *node_id,
            ));
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn sensitive_type(ty: &SemanticType) -> bool {
    matches!(
        classify_semantic_type(ty),
        DataClass::Secret | DataClass::Credential
    )
}

fn choose_sampling(
    mode: InstrumentationMode,
    budget: InstrumentationBudget,
) -> u32 {
    if mode == InstrumentationMode::Full {
        return 1;
    }

    let cost_per_second = (budget.estimated_probe_ns as u128)
        .saturating_mul(budget.expected_calls_per_s as u128);
    let allowed = (1_000_000_000_u128)
        .saturating_mul(budget.max_overhead_ppm as u128)
        / 1_000_000;

    if allowed == 0 {
        return u32::MAX;
    }

    cost_per_second
        .checked_div(allowed)
        .unwrap_or(u128::MAX)
        .max(1)
        .min(u32::MAX as u128) as u32
}

fn estimate_overhead_ppm(
    sample_every: u32,
    budget: InstrumentationBudget,
) -> u32 {
    if sample_every == 0 {
        return u32::MAX;
    }
    let probes = (budget.expected_calls_per_s as u128)
        .div_ceil(sample_every as u128);
    let ns = probes.saturating_mul(budget.estimated_probe_ns as u128);

    ns.saturating_mul(1_000_000)
        .checked_div(1_000_000_000)
        .unwrap_or(u128::MAX)
        .min(u32::MAX as u128) as u32
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{Node, Operation, Port};

    #[test]
    fn secret_node_is_profiled_without_value_capture() {
        let mut graph = Graph::new("auth");
        graph.nodes.push(Node {
            id: 1,
            operation: Operation::Subgraph("verify".into()),
            inputs: vec![Port {
                id: 0,
                name: "secret".into(),
                ty: SemanticType::Secret(Box::new(SemanticType::Bytes)),
            }],
            outputs: vec![],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        });

        let plan = build_instrumentation_plan(
            &graph,
            InstrumentationMode::Full,
            InstrumentationBudget {
                max_overhead_ppm: 100_000,
                estimated_probe_ns: 10,
                expected_calls_per_s: 1000,
            },
        )
        .unwrap();

        assert!(!plan.nodes[&1].capture_values);
        assert!(plan.nodes[&1]
            .metrics
            .contains(&InstrumentMetric::CpuTime));
    }
}
