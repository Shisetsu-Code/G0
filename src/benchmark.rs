#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenchmarkPlan {
    pub warmup_runs: u32,
    pub measured_runs: u32,
    pub max_noise_ppm: u32,
}

impl Default for BenchmarkPlan {
    fn default() -> Self {
        Self {
            warmup_runs: 5,
            measured_runs: 30,
            max_noise_ppm: 100_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BenchmarkIssue {
    ZeroMeasuredRuns,
    TooFewSamples {
        expected: usize,
        actual: usize,
    },
    ExcessiveNoise {
        noise_ppm: u32,
        maximum_ppm: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenchmarkStats {
    pub min_ns: u128,
    pub median_ns: u128,
    pub p95_ns: u128,
    pub p99_ns: u128,
    pub max_ns: u128,
    pub noise_ppm: u32,
}

pub fn analyze_samples(
    plan: BenchmarkPlan,
    samples_ns: &[u128],
) -> Result<BenchmarkStats, Vec<BenchmarkIssue>> {
    let mut issues = Vec::new();

    if plan.measured_runs == 0 {
        issues.push(BenchmarkIssue::ZeroMeasuredRuns);
    }

    if samples_ns.len() < plan.measured_runs as usize {
        issues.push(BenchmarkIssue::TooFewSamples {
            expected: plan.measured_runs as usize,
            actual: samples_ns.len(),
        });
    }

    if !issues.is_empty() {
        return Err(issues);
    }

    let mut samples = samples_ns.to_vec();
    samples.sort_unstable();

    let min_ns = samples[0];
    let max_ns = *samples.last().expect("non-empty");
    let median_ns = percentile(&samples, 50);
    let p95_ns = percentile(&samples, 95);
    let p99_ns = percentile(&samples, 99);

    let spread = p95_ns.saturating_sub(median_ns);
    let noise_ppm = if median_ns == 0 {
        if spread == 0 { 0 } else { u32::MAX }
    } else {
        spread
            .saturating_mul(1_000_000)
            .checked_div(median_ns)
            .unwrap_or(u128::MAX)
            .min(u32::MAX as u128) as u32
    };

    if noise_ppm > plan.max_noise_ppm {
        return Err(vec![BenchmarkIssue::ExcessiveNoise {
            noise_ppm,
            maximum_ppm: plan.max_noise_ppm,
        }]);
    }

    Ok(BenchmarkStats {
        min_ns,
        median_ns,
        p95_ns,
        p99_ns,
        max_ns,
        noise_ppm,
    })
}

fn percentile(sorted: &[u128], percentile: usize) -> u128 {
    let last = sorted.len() - 1;
    let index = (last * percentile).div_ceil(100);
    sorted[index.min(last)]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkIdentity {
    pub workload_hash: String,
    pub hardware_hash: String,
    pub build_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BenchmarkResult {
    pub identity: BenchmarkIdentity,
    pub stats: BenchmarkStats,
}

pub fn comparable(
    a: &BenchmarkResult,
    b: &BenchmarkResult,
) -> bool {
    a.identity == b.identity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_samples_produce_percentiles() {
        let plan = BenchmarkPlan {
            warmup_runs: 2,
            measured_runs: 5,
            max_noise_ppm: 200_000,
        };
        let stats =
            analyze_samples(plan, &[100, 101, 102, 103, 104]).unwrap();

        assert_eq!(stats.median_ns, 102);
        assert_eq!(stats.p99_ns, 104);
    }

    #[test]
    fn noisy_candidate_is_not_trusted() {
        let plan = BenchmarkPlan {
            warmup_runs: 2,
            measured_runs: 5,
            max_noise_ppm: 10_000,
        };

        assert!(matches!(
            analyze_samples(plan, &[100, 100, 100, 100, 1000]),
            Err(issues) if matches!(
                issues.first(),
                Some(BenchmarkIssue::ExcessiveNoise { .. })
            )
        ));
    }

    #[test]
    fn results_from_different_hardware_are_not_compared() {
        let stats = BenchmarkStats {
            min_ns: 1,
            median_ns: 1,
            p95_ns: 1,
            p99_ns: 1,
            max_ns: 1,
            noise_ppm: 0,
        };
        let a = BenchmarkResult {
            identity: BenchmarkIdentity {
                workload_hash: "w".into(),
                hardware_hash: "a".into(),
                build_hash: "b".into(),
            },
            stats,
        };
        let b = BenchmarkResult {
            identity: BenchmarkIdentity {
                workload_hash: "w".into(),
                hardware_hash: "other".into(),
                build_hash: "b".into(),
            },
            stats,
        };

        assert!(!comparable(&a, &b));
    }
}
