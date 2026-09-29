use std::collections::BTreeMap;

use crate::compiler::CompiledGraph;
use crate::profiling::{CostMetric, ProfileReport};
use crate::tuning::{Candidate, MetricName, ParameterName};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeMeasurement {
    pub p99_ns: i128,
    pub throughput_per_s: i128,
    pub energy_uj: Option<i128>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedbackIssue {
    MetricOverflow(&'static str),
    NegativeMeasurement(&'static str),
}

pub fn build_candidate_feedback(
    parameters: BTreeMap<ParameterName, i128>,
    compiled: &CompiledGraph,
    profile: &ProfileReport,
    runtime: &RuntimeMeasurement,
    invariants: Vec<String>,
) -> Result<Candidate, Vec<FeedbackIssue>> {
    let mut issues = Vec::new();

    if runtime.p99_ns < 0 {
        issues.push(FeedbackIssue::NegativeMeasurement("p99_ns"));
    }
    if runtime.throughput_per_s < 0 {
        issues.push(FeedbackIssue::NegativeMeasurement(
            "throughput_per_s",
        ));
    }

    let mut metrics = BTreeMap::new();
    metrics.insert(MetricName::new("p99_ns"), runtime.p99_ns);
    metrics.insert(
        MetricName::new("throughput_per_s"),
        runtime.throughput_per_s,
    );

    add_u128_metric(
        &mut metrics,
        "cpu_ns",
        aggregate(profile, CostMetric::CpuTime),
        &mut issues,
    );
    add_u128_metric(
        &mut metrics,
        "queue_ns",
        aggregate(profile, CostMetric::QueueTime),
        &mut issues,
    );
    add_u128_metric(
        &mut metrics,
        "io_wait_ns",
        aggregate(profile, CostMetric::IoWaitTime),
        &mut issues,
    );
    add_u128_metric(
        &mut metrics,
        "allocated_bytes",
        aggregate(profile, CostMetric::AllocatedBytes),
        &mut issues,
    );
    add_u128_metric(
        &mut metrics,
        "network_bytes",
        aggregate(profile, CostMetric::NetworkBytes),
        &mut issues,
    );
    add_u128_metric(
        &mut metrics,
        "storage_bytes",
        aggregate(profile, CostMetric::StorageBytes),
        &mut issues,
    );

    metrics.insert(
        MetricName::new("spill_count"),
        compiled.pressure.spills as i128,
    );
    metrics.insert(
        MetricName::new("stack_bytes"),
        compiled.pressure.stack_bytes as i128,
    );
    metrics.insert(
        MetricName::new("peak_live_values"),
        compiled.pressure.peak_live_values as i128,
    );

    if let Some(energy) = runtime.energy_uj {
        if energy < 0 {
            issues.push(FeedbackIssue::NegativeMeasurement("energy_uj"));
        } else {
            metrics.insert(MetricName::new("energy_uj"), energy);
        }
    }

    if issues.is_empty() {
        Ok(Candidate {
            parameters,
            metrics,
            passed_invariants: invariants,
        })
    } else {
        Err(issues)
    }
}

fn aggregate(report: &ProfileReport, metric: CostMetric) -> u128 {
    report
        .rank_hotspots(metric, usize::MAX)
        .iter()
        .map(|hotspot| hotspot.value)
        .sum()
}

fn add_u128_metric(
    metrics: &mut BTreeMap<MetricName, i128>,
    name: &'static str,
    value: u128,
    issues: &mut Vec<FeedbackIssue>,
) {
    match i128::try_from(value) {
        Ok(value) => {
            metrics.insert(MetricName::new(name), value);
        }
        Err(_) => issues.push(FeedbackIssue::MetricOverflow(name)),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::compiler::PhysicalCostSignal;
    use crate::invariants::InvariantLedger;
    use crate::machine::{AllocationResult, RegisterPressure};
    use crate::machine_ir::MachineProgram;
    use crate::mir::MirProgram;
    use crate::profiling::{NodeLocation, NodeMetrics, NodeProfile};

    #[test]
    fn compiler_and_runtime_metrics_feed_same_candidate() {
        let compiled = CompiledGraph {
            graph_name: "main".into(),
            mir: MirProgram::default(),
            allocation: AllocationResult::default(),
            pressure: RegisterPressure {
                peak_live_values: 7,
                allocated_register_values: 6,
                spills: 1,
                stack_bytes: 32,
            },
            machine_ir: MachineProgram::default(),
            assembly: String::new(),
            invariants: InvariantLedger::default(),
        };

        assert_eq!(
            compiled.cost_signal(PhysicalCostSignal::SpillCount),
            1
        );

        let profile = ProfileReport {
            nodes: BTreeMap::from([(
                1,
                NodeProfile {
                    location: NodeLocation {
                        graph: "main".into(),
                        node: 1,
                        label: "work".into(),
                    },
                    metrics: NodeMetrics {
                        cpu_ns_total: 100,
                        allocated_bytes: 200,
                        ..NodeMetrics::default()
                    },
                },
            )]),
        };

        let candidate = build_candidate_feedback(
            BTreeMap::new(),
            &compiled,
            &profile,
            &RuntimeMeasurement {
                p99_ns: 1000,
                throughput_per_s: 50_000,
                energy_uj: None,
            },
            vec!["security.high".into()],
        )
        .unwrap();

        assert_eq!(
            candidate.metrics[&MetricName::new("spill_count")],
            1
        );
        assert_eq!(
            candidate.metrics[&MetricName::new("cpu_ns")],
            100
        );
    }
}
