use crate::invariants::{
    validate_preservation, InvariantLedger, PreservationIssue,
};
use crate::tuning::{
    validate_candidate, Candidate, CandidateIssue, OptimizationProfile,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptimizationCandidate {
    pub candidate: Candidate,
    pub baseline_invariants: InvariantLedger,
    pub resulting_invariants: InvariantLedger,
    pub transformation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OptimizationGateIssue {
    Candidate(Vec<CandidateIssue>),
    InvariantPreservation(Vec<PreservationIssue>),
    ResultContainsInvalidInvariant,
}

pub fn validate_optimization_candidate(
    profile: &OptimizationProfile,
    candidate: &OptimizationCandidate,
) -> Result<(), Vec<OptimizationGateIssue>> {
    let mut issues = Vec::new();

    if let Err(candidate_issues) =
        validate_candidate(profile, &candidate.candidate)
    {
        issues.push(OptimizationGateIssue::Candidate(candidate_issues));
    }

    if let Err(preservation_issues) = validate_preservation(
        &candidate.baseline_invariants,
        &candidate.resulting_invariants,
    ) {
        issues.push(OptimizationGateIssue::InvariantPreservation(
            preservation_issues,
        ));
    }

    if !candidate.resulting_invariants.compilation_allowed() {
        issues.push(
            OptimizationGateIssue::ResultContainsInvalidInvariant,
        );
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn benchmark_allowed(
    profile: &OptimizationProfile,
    candidate: &OptimizationCandidate,
) -> bool {
    validate_optimization_candidate(profile, candidate).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gir::ParameterPolicy;
    use crate::invariants::{
        InvariantKey, InvariantKind, InvariantStatus,
    };
    use crate::tuning::{ParameterName, SearchParameter};

    fn ledger(status: InvariantStatus) -> InvariantLedger {
        let mut ledger = InvariantLedger::default();
        ledger.record(
            InvariantKey {
                graph: "main".into(),
                node: Some(1),
                kind: InvariantKind::IntegerRange,
            },
            status,
        );
        ledger
    }

    #[test]
    fn fast_candidate_that_loses_proof_cannot_be_benchmarked() {
        let profile = OptimizationProfile {
            parameters: vec![SearchParameter {
                name: ParameterName::new("workers"),
                policy: ParameterPolicy::Bounded { min: 1, max: 32 },
            }],
            ..OptimizationProfile::default()
        };
        let mut raw = Candidate::default();
        raw.parameters.insert(ParameterName::new("workers"), 32);

        let candidate = OptimizationCandidate {
            candidate: raw,
            baseline_invariants: ledger(InvariantStatus::Proven),
            resulting_invariants: InvariantLedger::default(),
            transformation: "parallelize".into(),
        };

        assert!(!benchmark_allowed(&profile, &candidate));
    }

    #[test]
    fn runtime_check_is_valid_preservation_of_a_proof() {
        let profile = OptimizationProfile::default();
        let candidate = OptimizationCandidate {
            candidate: Candidate::default(),
            baseline_invariants: ledger(InvariantStatus::Proven),
            resulting_invariants: ledger(
                InvariantStatus::RuntimeCheckRequired,
            ),
            transformation: "specialize".into(),
        };

        assert!(benchmark_allowed(&profile, &candidate));
    }
}
