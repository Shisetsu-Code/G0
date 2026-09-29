use std::collections::BTreeMap;

use crate::gir::ParameterPolicy;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ParameterName(pub String);

impl ParameterName {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchParameter {
    pub name: ParameterName,
    pub policy: ParameterPolicy<i128>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct MetricName(pub String);

impl MetricName {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectiveDirection {
    Minimize,
    Maximize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Objective {
    pub metric: MetricName,
    pub direction: ObjectiveDirection,
    /// Lower numbers have higher priority.
    pub priority: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardConstraint {
    MetricAtMost { metric: MetricName, value: i128 },
    MetricAtLeast { metric: MetricName, value: i128 },
    Invariant(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OptimizationProfile {
    pub parameters: Vec<SearchParameter>,
    pub constraints: Vec<HardConstraint>,
    pub objectives: Vec<Objective>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Candidate {
    pub parameters: BTreeMap<ParameterName, i128>,
    pub metrics: BTreeMap<MetricName, i128>,
    pub passed_invariants: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateIssue {
    MissingParameter(ParameterName),
    FixedParameterChanged {
        parameter: ParameterName,
        required: i128,
        actual: i128,
    },
    ParameterOutOfBounds {
        parameter: ParameterName,
        min: i128,
        max: i128,
        actual: i128,
    },
    MissingMetric(MetricName),
    ConstraintFailed {
        metric: MetricName,
        required: String,
        actual: i128,
    },
    MissingInvariant(String),
}

pub fn validate_candidate(
    profile: &OptimizationProfile,
    candidate: &Candidate,
) -> Result<(), Vec<CandidateIssue>> {
    let mut issues = Vec::new();

    for parameter in &profile.parameters {
        let Some(actual) = candidate.parameters.get(&parameter.name).copied() else {
            issues.push(CandidateIssue::MissingParameter(parameter.name.clone()));
            continue;
        };

        match parameter.policy {
            ParameterPolicy::Fixed(required) if actual != required => {
                issues.push(CandidateIssue::FixedParameterChanged {
                    parameter: parameter.name.clone(),
                    required,
                    actual,
                });
            }
            ParameterPolicy::Bounded { min, max } if actual < min || actual > max => {
                issues.push(CandidateIssue::ParameterOutOfBounds {
                    parameter: parameter.name.clone(),
                    min,
                    max,
                    actual,
                });
            }
            ParameterPolicy::Fixed(_) | ParameterPolicy::Bounded { .. } | ParameterPolicy::Free => {
            }
        }
    }

    for constraint in &profile.constraints {
        match constraint {
            HardConstraint::MetricAtMost { metric, value } => {
                validate_metric_bound(candidate, metric, *value, true, &mut issues);
            }
            HardConstraint::MetricAtLeast { metric, value } => {
                validate_metric_bound(candidate, metric, *value, false, &mut issues);
            }
            HardConstraint::Invariant(name) => {
                if !candidate
                    .passed_invariants
                    .iter()
                    .any(|value| value == name)
                {
                    issues.push(CandidateIssue::MissingInvariant(name.clone()));
                }
            }
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn validate_metric_bound(
    candidate: &Candidate,
    metric: &MetricName,
    bound: i128,
    is_max: bool,
    issues: &mut Vec<CandidateIssue>,
) {
    let Some(actual) = candidate.metrics.get(metric).copied() else {
        issues.push(CandidateIssue::MissingMetric(metric.clone()));
        return;
    };

    let failed = if is_max {
        actual > bound
    } else {
        actual < bound
    };

    if failed {
        let required = if is_max {
            format!("<= {bound}")
        } else {
            format!(">= {bound}")
        };
        issues.push(CandidateIssue::ConstraintFailed {
            metric: metric.clone(),
            required,
            actual,
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptimizationLock {
    pub program_hash: String,
    pub compiler_version: String,
    pub hardware_profile_hash: String,
    pub workload_hash: String,
    pub selected_parameters: BTreeMap<ParameterName, i128>,
    pub measured_metrics: BTreeMap<MetricName, i128>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_security_floor_parameter_cannot_be_mutated() {
        let profile = OptimizationProfile {
            parameters: vec![SearchParameter {
                name: ParameterName::new("security.minimum"),
                policy: ParameterPolicy::Fixed(3),
            }],
            ..OptimizationProfile::default()
        };

        let mut candidate = Candidate::default();
        candidate
            .parameters
            .insert(ParameterName::new("security.minimum"), 2);

        assert!(matches!(
            validate_candidate(&profile, &candidate)
                .unwrap_err()
                .first(),
            Some(CandidateIssue::FixedParameterChanged { .. })
        ));
    }

    #[test]
    fn password_memory_can_be_tuned_inside_bounds() {
        let profile = OptimizationProfile {
            parameters: vec![SearchParameter {
                name: ParameterName::new("password.memory_mib"),
                policy: ParameterPolicy::Bounded {
                    min: 128,
                    max: 1024,
                },
            }],
            ..OptimizationProfile::default()
        };

        let mut candidate = Candidate::default();
        candidate
            .parameters
            .insert(ParameterName::new("password.memory_mib"), 512);

        assert!(validate_candidate(&profile, &candidate).is_ok());
    }

    #[test]
    fn fast_but_insecure_candidate_is_discarded_before_scoring() {
        let profile = OptimizationProfile {
            constraints: vec![
                HardConstraint::Invariant("security.high".into()),
                HardConstraint::MetricAtMost {
                    metric: MetricName::new("p99_ns"),
                    value: 250_000_000,
                },
            ],
            objectives: vec![Objective {
                metric: MetricName::new("throughput_per_s"),
                direction: ObjectiveDirection::Maximize,
                priority: 0,
            }],
            ..OptimizationProfile::default()
        };

        let mut candidate = Candidate::default();
        candidate
            .metrics
            .insert(MetricName::new("p99_ns"), 1_000_000);
        candidate
            .metrics
            .insert(MetricName::new("throughput_per_s"), 1_000_000);

        assert!(matches!(
            validate_candidate(&profile, &candidate)
                .unwrap_err()
                .first(),
            Some(CandidateIssue::MissingInvariant(name)) if name == "security.high"
        ));
    }
}
