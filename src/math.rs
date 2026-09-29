use crate::gir::ParameterPolicy;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RoundingMode {
    NearestEven,
    TowardZero,
    TowardPositive,
    TowardNegative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MathMode {
    /// Strong default: bounded error, no unsafe algebraic rewrites.
    Strict,
    /// Same input/profile must produce the same result across supported targets.
    Deterministic,
    /// Precision is intentionally higher than native machine precision.
    HighPrecision,
    /// Result must be rounded according to the declared target precision/mode.
    CorrectlyRounded,
    /// Exact result only where the mathematical domain permits exact representation.
    Exact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MathOperation {
    IntegerAdd,
    IntegerSub,
    IntegerMul,
    RationalAdd,
    RationalSub,
    RationalMul,
    RationalDiv,
    Sqrt,
    Exp,
    Ln,
    Sin,
    Cos,
    Tan,
    Pow,
}

impl MathOperation {
    pub fn exact_domain_supported(self) -> bool {
        matches!(
            self,
            Self::IntegerAdd
                | Self::IntegerSub
                | Self::IntegerMul
                | Self::RationalAdd
                | Self::RationalSub
                | Self::RationalMul
                | Self::RationalDiv
        )
    }

    pub fn transcendental(self) -> bool {
        matches!(
            self,
            Self::Exp | Self::Ln | Self::Sin | Self::Cos | Self::Tan | Self::Pow
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MathRequirement {
    pub mode: MathMode,
    pub precision_bits: ParameterPolicy<u32>,
    pub rounding: RoundingMode,
    pub max_relative_error_ppb: Option<u64>,
    pub allow_reassociation: bool,
}

impl MathRequirement {
    pub fn strict(precision_bits: u32) -> Self {
        Self {
            mode: MathMode::Strict,
            precision_bits: ParameterPolicy::Fixed(precision_bits),
            rounding: RoundingMode::NearestEven,
            max_relative_error_ppb: None,
            allow_reassociation: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathProfile {
    pub id: String,
    pub max_precision_bits: u32,
    pub deterministic: bool,
    pub correctly_rounded_transcendentals: bool,
    pub supported_rounding: Vec<RoundingMode>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathContractIssue {
    PrecisionExceedsPlatform {
        requested_bits: u32,
        platform_max_bits: u32,
    },
    FreePrecisionHasNoLowerBound,
    DeterminismUnavailable,
    CorrectRoundingUnavailable(MathOperation),
    ExactModeInvalidForOperation(MathOperation),
    RoundingModeUnavailable(RoundingMode),
    ReassociationForbiddenByMode,
}

pub fn validate_math_requirement(
    profile: &MathProfile,
    operation: MathOperation,
    requirement: MathRequirement,
) -> Result<(), Vec<MathContractIssue>> {
    let mut issues = Vec::new();

    match requirement.precision_bits {
        ParameterPolicy::Fixed(bits) => {
            if bits > profile.max_precision_bits {
                issues.push(MathContractIssue::PrecisionExceedsPlatform {
                    requested_bits: bits,
                    platform_max_bits: profile.max_precision_bits,
                });
            }
        }
        ParameterPolicy::Bounded { min, max } => {
            if max > profile.max_precision_bits {
                issues.push(MathContractIssue::PrecisionExceedsPlatform {
                    requested_bits: max,
                    platform_max_bits: profile.max_precision_bits,
                });
            }
            if min == 0 {
                issues.push(MathContractIssue::FreePrecisionHasNoLowerBound);
            }
        }
        ParameterPolicy::Free => {
            issues.push(MathContractIssue::FreePrecisionHasNoLowerBound);
        }
    }

    if requirement.mode == MathMode::Deterministic && !profile.deterministic {
        issues.push(MathContractIssue::DeterminismUnavailable);
    }

    if requirement.mode == MathMode::CorrectlyRounded
        && operation.transcendental()
        && !profile.correctly_rounded_transcendentals
    {
        issues.push(MathContractIssue::CorrectRoundingUnavailable(operation));
    }

    if requirement.mode == MathMode::Exact && !operation.exact_domain_supported() {
        issues.push(MathContractIssue::ExactModeInvalidForOperation(operation));
    }

    if !profile.supported_rounding.contains(&requirement.rounding) {
        issues.push(MathContractIssue::RoundingModeUnavailable(
            requirement.rounding,
        ));
    }

    if requirement.allow_reassociation
        && matches!(
            requirement.mode,
            MathMode::Deterministic | MathMode::CorrectlyRounded | MathMode::Exact
        )
    {
        issues.push(MathContractIssue::ReassociationForbiddenByMode);
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApproximationCertificate {
    /// Guaranteed working/result precision.
    pub precision_bits: u32,
    /// Optional measured/proven relative error bound in parts per billion.
    pub relative_error_ppb: Option<u64>,
    pub rounding: RoundingMode,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> MathProfile {
        MathProfile {
            id: "math.current".into(),
            max_precision_bits: 4096,
            deterministic: true,
            correctly_rounded_transcendentals: true,
            supported_rounding: vec![
                RoundingMode::NearestEven,
                RoundingMode::TowardZero,
                RoundingMode::TowardPositive,
                RoundingMode::TowardNegative,
            ],
        }
    }

    #[test]
    fn exact_integer_math_is_valid() {
        let requirement = MathRequirement {
            mode: MathMode::Exact,
            precision_bits: ParameterPolicy::Fixed(256),
            rounding: RoundingMode::NearestEven,
            max_relative_error_ppb: Some(0),
            allow_reassociation: false,
        };

        assert!(
            validate_math_requirement(
                &profile(),
                MathOperation::IntegerMul,
                requirement,
            )
            .is_ok()
        );
    }

    #[test]
    fn exact_sine_is_rejected_as_an_invalid_contract() {
        let requirement = MathRequirement {
            mode: MathMode::Exact,
            precision_bits: ParameterPolicy::Fixed(256),
            rounding: RoundingMode::NearestEven,
            max_relative_error_ppb: Some(0),
            allow_reassociation: false,
        };

        assert_eq!(
            validate_math_requirement(
                &profile(),
                MathOperation::Sin,
                requirement,
            ),
            Err(vec![MathContractIssue::ExactModeInvalidForOperation(
                MathOperation::Sin
            )])
        );
    }

    #[test]
    fn high_precision_can_be_tunable_but_needs_a_floor() {
        let requirement = MathRequirement {
            mode: MathMode::HighPrecision,
            precision_bits: ParameterPolicy::Bounded {
                min: 256,
                max: 2048,
            },
            rounding: RoundingMode::NearestEven,
            max_relative_error_ppb: Some(1),
            allow_reassociation: false,
        };

        assert!(
            validate_math_requirement(
                &profile(),
                MathOperation::Exp,
                requirement,
            )
            .is_ok()
        );
    }

    #[test]
    fn deterministic_mode_disallows_reassociation() {
        let requirement = MathRequirement {
            mode: MathMode::Deterministic,
            precision_bits: ParameterPolicy::Fixed(128),
            rounding: RoundingMode::NearestEven,
            max_relative_error_ppb: None,
            allow_reassociation: true,
        };

        assert!(
            validate_math_requirement(
                &profile(),
                MathOperation::RationalAdd,
                requirement,
            )
            .unwrap_err()
            .contains(&MathContractIssue::ReassociationForbiddenByMode)
        );
    }
}
