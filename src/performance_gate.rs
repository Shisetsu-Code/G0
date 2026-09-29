#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerformanceSnapshot {
    pub p99_ns: u128,
    pub throughput_per_s: u128,
    pub peak_memory_bytes: u128,
    pub allocated_bytes: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegressionPolicy {
    pub max_p99_regression_ppm: u32,
    pub max_throughput_regression_ppm: u32,
    pub max_peak_memory_regression_ppm: u32,
    pub max_allocation_regression_ppm: u32,
}

impl Default for RegressionPolicy {
    fn default() -> Self {
        Self {
            max_p99_regression_ppm: 50_000,
            max_throughput_regression_ppm: 50_000,
            max_peak_memory_regression_ppm: 100_000,
            max_allocation_regression_ppm: 100_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegressionIssue {
    P99 { regression_ppm: u32 },
    Throughput { regression_ppm: u32 },
    PeakMemory { regression_ppm: u32 },
    AllocatedBytes { regression_ppm: u32 },
}

pub fn validate_regression(
    baseline: PerformanceSnapshot,
    candidate: PerformanceSnapshot,
    policy: RegressionPolicy,
) -> Result<(), Vec<RegressionIssue>> {
    let mut issues = Vec::new();

    let p99 = increase_ppm(baseline.p99_ns, candidate.p99_ns);
    if p99 > policy.max_p99_regression_ppm {
        issues.push(RegressionIssue::P99 {
            regression_ppm: p99,
        });
    }

    let throughput =
        decrease_ppm(baseline.throughput_per_s, candidate.throughput_per_s);
    if throughput > policy.max_throughput_regression_ppm {
        issues.push(RegressionIssue::Throughput {
            regression_ppm: throughput,
        });
    }

    let memory =
        increase_ppm(baseline.peak_memory_bytes, candidate.peak_memory_bytes);
    if memory > policy.max_peak_memory_regression_ppm {
        issues.push(RegressionIssue::PeakMemory {
            regression_ppm: memory,
        });
    }

    let allocated =
        increase_ppm(baseline.allocated_bytes, candidate.allocated_bytes);
    if allocated > policy.max_allocation_regression_ppm {
        issues.push(RegressionIssue::AllocatedBytes {
            regression_ppm: allocated,
        });
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn increase_ppm(baseline: u128, candidate: u128) -> u32 {
    if candidate <= baseline {
        return 0;
    }
    ratio_ppm(candidate - baseline, baseline)
}

fn decrease_ppm(baseline: u128, candidate: u128) -> u32 {
    if candidate >= baseline {
        return 0;
    }
    ratio_ppm(baseline - candidate, baseline)
}

fn ratio_ppm(delta: u128, baseline: u128) -> u32 {
    if baseline == 0 {
        return if delta == 0 { 0 } else { u32::MAX };
    }
    delta
        .saturating_mul(1_000_000)
        .checked_div(baseline)
        .unwrap_or(u128::MAX)
        .min(u32::MAX as u128) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn baseline() -> PerformanceSnapshot {
        PerformanceSnapshot {
            p99_ns: 100,
            throughput_per_s: 1000,
            peak_memory_bytes: 1000,
            allocated_bytes: 1000,
        }
    }

    #[test]
    fn small_tradeoffs_inside_policy_are_allowed() {
        let candidate = PerformanceSnapshot {
            p99_ns: 104,
            throughput_per_s: 970,
            peak_memory_bytes: 1050,
            allocated_bytes: 1050,
        };

        assert!(
            validate_regression(
                baseline(),
                candidate,
                RegressionPolicy::default(),
            )
            .is_ok()
        );
    }

    #[test]
    fn large_latency_regression_is_rejected() {
        let candidate = PerformanceSnapshot {
            p99_ns: 120,
            ..baseline()
        };

        assert!(validate_regression(
            baseline(),
            candidate,
            RegressionPolicy::default(),
        )
        .unwrap_err()
        .iter()
        .any(|issue| matches!(issue, RegressionIssue::P99 { .. })));
    }
}
