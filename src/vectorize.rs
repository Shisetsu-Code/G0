use crate::hardware::{preferred_vector_bits, HardwareProfile};
use crate::memory::IntegerWidth;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScalarKind {
    Integer(IntegerWidth),
    Float32,
    Float64,
}

impl ScalarKind {
    pub fn bits(self) -> u16 {
        match self {
            Self::Integer(width) => width.bits(),
            Self::Float32 => 32,
            Self::Float64 => 64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorSemantics {
    Exact,
    BoundedFloatError { max_relative_error_ppb: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorizationRequest {
    pub scalar: ScalarKind,
    pub element_count: usize,
    pub semantics: VectorSemantics,
    pub allow_reassociation: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VectorPlan {
    pub vector_bits: u16,
    pub lanes: u16,
    pub full_vectors: usize,
    pub tail_elements: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VectorizationIssue {
    NoVectorHardware,
    LaneTooWide {
        scalar_bits: u16,
        vector_bits: u16,
    },
    EmptyWorkload,
    ExactFloatReassociationForbidden,
}

pub fn plan_vectorization(
    hardware: &HardwareProfile,
    request: &VectorizationRequest,
) -> Result<VectorPlan, VectorizationIssue> {
    if request.element_count == 0 {
        return Err(VectorizationIssue::EmptyWorkload);
    }

    let vector_bits = preferred_vector_bits(hardware);
    if vector_bits == 0 {
        return Err(VectorizationIssue::NoVectorHardware);
    }

    let scalar_bits = request.scalar.bits();
    if scalar_bits > vector_bits {
        return Err(VectorizationIssue::LaneTooWide {
            scalar_bits,
            vector_bits,
        });
    }

    if request.allow_reassociation
        && matches!(
            request.semantics,
            VectorSemantics::Exact
        )
        && matches!(
            request.scalar,
            ScalarKind::Float32 | ScalarKind::Float64
        )
    {
        return Err(
            VectorizationIssue::ExactFloatReassociationForbidden,
        );
    }

    let lanes = vector_bits / scalar_bits;
    let lanes_usize = lanes as usize;

    Ok(VectorPlan {
        vector_bits,
        lanes,
        full_vectors: request.element_count / lanes_usize,
        tail_elements: request.element_count % lanes_usize,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TailStrategy {
    Scalar,
    Masked,
}

pub fn tail_strategy(
    plan: VectorPlan,
    hardware: &HardwareProfile,
) -> TailStrategy {
    if plan.tail_elements == 0 {
        TailStrategy::Scalar
    } else if preferred_vector_bits(hardware) >= 512 {
        TailStrategy::Masked
    } else {
        TailStrategy::Scalar
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::hardware::{
        CacheProfile, HardwareFeature, HardwareProfile,
    };

    fn hardware() -> HardwareProfile {
        HardwareProfile {
            id: "modern".into(),
            logical_cores: 8,
            physical_cores: 8,
            memory_bytes: 16 * 1024 * 1024 * 1024,
            numa_nodes: 1,
            cache: CacheProfile {
                line_bytes: 64,
                l1_data_bytes: 32 * 1024,
                l2_bytes_per_core: 1024 * 1024,
                l3_bytes_total: 16 * 1024 * 1024,
            },
            features: BTreeSet::from([
                HardwareFeature::Vector128,
                HardwareFeature::Vector256,
            ]),
        }
    }

    #[test]
    fn u32_workload_uses_eight_lanes_on_256_bit_target() {
        let plan = plan_vectorization(
            &hardware(),
            &VectorizationRequest {
                scalar: ScalarKind::Integer(IntegerWidth::U32),
                element_count: 1000,
                semantics: VectorSemantics::Exact,
                allow_reassociation: false,
            },
        )
        .unwrap();

        assert_eq!(plan.vector_bits, 256);
        assert_eq!(plan.lanes, 8);
        assert_eq!(plan.full_vectors, 125);
        assert_eq!(plan.tail_elements, 0);
    }

    #[test]
    fn exact_float_reassociation_is_rejected() {
        assert_eq!(
            plan_vectorization(
                &hardware(),
                &VectorizationRequest {
                    scalar: ScalarKind::Float32,
                    element_count: 1024,
                    semantics: VectorSemantics::Exact,
                    allow_reassociation: true,
                },
            ),
            Err(
                VectorizationIssue::ExactFloatReassociationForbidden
            )
        );
    }
}
