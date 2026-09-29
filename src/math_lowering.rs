use crate::gir::{DecimalType, SemanticType};
use crate::math::{MathMode, MathOperation, MathRequirement};
use crate::memory::{choose_integer_width, IntegerWidth};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhysicalNumericType {
    Integer(IntegerWidth),
    Float32,
    Float64,
    Decimal {
        precision_digits: u32,
        scale: i32,
    },
    BigInteger,
    Rational,
    BigFloat {
        precision_bits: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathBackend {
    NativeScalar,
    NativeVectorizable,
    SoftwareExact,
    SoftwareHighPrecision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MathLoweringPlan {
    pub physical_type: PhysicalNumericType,
    pub backend: MathBackend,
    pub runtime_rounding_check: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MathLoweringIssue {
    NotNumeric,
    FreePrecisionNotLowerable,
    PrecisionRangeInvalid,
    ExactFloatOperationUnsupported(MathOperation),
}

pub fn plan_math_lowering(
    ty: &SemanticType,
    operation: MathOperation,
    requirement: MathRequirement,
) -> Result<MathLoweringPlan, MathLoweringIssue> {
    let precision = selected_precision(requirement.precision_bits)?;

    match ty {
        SemanticType::Integer(range) => Ok(MathLoweringPlan {
            physical_type: PhysicalNumericType::Integer(
                choose_integer_width(range),
            ),
            backend: MathBackend::NativeScalar,
            runtime_rounding_check: false,
        }),
        SemanticType::BigInteger => Ok(MathLoweringPlan {
            physical_type: PhysicalNumericType::BigInteger,
            backend: MathBackend::SoftwareExact,
            runtime_rounding_check: false,
        }),
        SemanticType::Rational => Ok(MathLoweringPlan {
            physical_type: PhysicalNumericType::Rational,
            backend: MathBackend::SoftwareExact,
            runtime_rounding_check: false,
        }),
        SemanticType::Decimal(decimal) => decimal_plan(decimal),
        SemanticType::Float(_) => float_plan(operation, requirement, precision),
        SemanticType::BigFloat(big) => Ok(MathLoweringPlan {
            physical_type: PhysicalNumericType::BigFloat {
                precision_bits: big.precision_bits.max(precision),
            },
            backend: MathBackend::SoftwareHighPrecision,
            runtime_rounding_check: requirement.mode
                == MathMode::CorrectlyRounded,
        }),
        _ => Err(MathLoweringIssue::NotNumeric),
    }
}

fn decimal_plan(
    decimal: &DecimalType,
) -> Result<MathLoweringPlan, MathLoweringIssue> {
    Ok(MathLoweringPlan {
        physical_type: PhysicalNumericType::Decimal {
            precision_digits: decimal.precision_digits,
            scale: decimal.scale,
        },
        backend: MathBackend::SoftwareExact,
        runtime_rounding_check: false,
    })
}

fn float_plan(
    operation: MathOperation,
    requirement: MathRequirement,
    precision: u32,
) -> Result<MathLoweringPlan, MathLoweringIssue> {
    if requirement.mode == MathMode::Exact
        && !operation.exact_domain_supported()
    {
        return Err(MathLoweringIssue::ExactFloatOperationUnsupported(
            operation,
        ));
    }

    if requirement.mode == MathMode::Exact {
        return Ok(MathLoweringPlan {
            physical_type: PhysicalNumericType::BigFloat {
                precision_bits: precision.max(128),
            },
            backend: MathBackend::SoftwareHighPrecision,
            runtime_rounding_check: true,
        });
    }

    if precision <= 24 {
        Ok(MathLoweringPlan {
            physical_type: PhysicalNumericType::Float32,
            backend: MathBackend::NativeVectorizable,
            runtime_rounding_check: requirement.mode
                == MathMode::CorrectlyRounded,
        })
    } else if precision <= 53 {
        Ok(MathLoweringPlan {
            physical_type: PhysicalNumericType::Float64,
            backend: MathBackend::NativeVectorizable,
            runtime_rounding_check: requirement.mode
                == MathMode::CorrectlyRounded,
        })
    } else {
        Ok(MathLoweringPlan {
            physical_type: PhysicalNumericType::BigFloat {
                precision_bits: precision,
            },
            backend: MathBackend::SoftwareHighPrecision,
            runtime_rounding_check: requirement.mode
                == MathMode::CorrectlyRounded,
        })
    }
}

fn selected_precision(
    policy: crate::gir::ParameterPolicy<u32>,
) -> Result<u32, MathLoweringIssue> {
    match policy {
        crate::gir::ParameterPolicy::Fixed(value) => Ok(value),
        crate::gir::ParameterPolicy::Bounded { min, max } => {
            if min == 0 || min > max {
                Err(MathLoweringIssue::PrecisionRangeInvalid)
            } else {
                Ok(min)
            }
        }
        crate::gir::ParameterPolicy::Free => {
            Err(MathLoweringIssue::FreePrecisionNotLowerable)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gir::{BigFloatType, FloatType, ParameterPolicy};
    use crate::math::{RoundingMode};

    fn requirement(bits: u32) -> MathRequirement {
        MathRequirement {
            mode: MathMode::HighPrecision,
            precision_bits: ParameterPolicy::Fixed(bits),
            rounding: RoundingMode::NearestEven,
            max_relative_error_ppb: Some(1),
            allow_reassociation: false,
        }
    }

    #[test]
    fn normal_precision_uses_native_float() {
        let plan = plan_math_lowering(
            &SemanticType::Float(FloatType {
                max_relative_error_ppb: Some(1),
            }),
            MathOperation::Exp,
            requirement(53),
        )
        .unwrap();

        assert_eq!(plan.physical_type, PhysicalNumericType::Float64);
        assert_eq!(plan.backend, MathBackend::NativeVectorizable);
    }

    #[test]
    fn high_precision_uses_big_float_automatically() {
        let plan = plan_math_lowering(
            &SemanticType::BigFloat(BigFloatType::new(1024).unwrap()),
            MathOperation::Sin,
            requirement(512),
        )
        .unwrap();

        assert_eq!(
            plan.physical_type,
            PhysicalNumericType::BigFloat {
                precision_bits: 1024
            }
        );
        assert_eq!(plan.backend, MathBackend::SoftwareHighPrecision);
    }
}
