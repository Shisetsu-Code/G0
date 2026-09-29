use crate::hardware::{HardwareFeature, HardwareProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceleratorWorkKind {
    ElementWise,
    Reduction,
    Matrix,
    Transform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceleratorSemantics {
    Exact,
    BoundedError { max_relative_error_ppb: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcceleratorRequest {
    pub work: AcceleratorWorkKind,
    pub elements: u64,
    pub input_bytes: u64,
    pub output_bytes: u64,
    pub estimated_cpu_ns: u128,
    pub estimated_accelerator_ns: u128,
    pub transfer_bandwidth_bytes_per_s: u64,
    pub fixed_transfer_latency_ns: u64,
    pub semantics: AcceleratorSemantics,
    pub secret_data: bool,
    pub accelerator_secret_capable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionTarget {
    Cpu,
    Accelerator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AcceleratorPlan {
    pub target: ExecutionTarget,
    pub estimated_total_ns: u128,
    pub transfer_ns: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceleratorIssue {
    NoAccelerator,
    ZeroBandwidth,
    SecretDataNotAllowed,
    EmptyWorkload,
}

pub fn plan_acceleration(
    hardware: &HardwareProfile,
    request: &AcceleratorRequest,
) -> Result<AcceleratorPlan, AcceleratorIssue> {
    if request.elements == 0 {
        return Err(AcceleratorIssue::EmptyWorkload);
    }

    if !hardware.features.contains(&HardwareFeature::Accelerator) {
        return Ok(AcceleratorPlan {
            target: ExecutionTarget::Cpu,
            estimated_total_ns: request.estimated_cpu_ns,
            transfer_ns: 0,
        });
    }

    if request.transfer_bandwidth_bytes_per_s == 0 {
        return Err(AcceleratorIssue::ZeroBandwidth);
    }

    if request.secret_data && !request.accelerator_secret_capable {
        return Err(AcceleratorIssue::SecretDataNotAllowed);
    }

    let transfer_bytes =
        request.input_bytes as u128 + request.output_bytes as u128;
    let transfer_ns = transfer_bytes
        .saturating_mul(1_000_000_000)
        .checked_div(request.transfer_bandwidth_bytes_per_s as u128)
        .unwrap_or(u128::MAX)
        .saturating_add(request.fixed_transfer_latency_ns as u128);

    let accelerator_total =
        request.estimated_accelerator_ns.saturating_add(transfer_ns);

    if accelerator_total < request.estimated_cpu_ns {
        Ok(AcceleratorPlan {
            target: ExecutionTarget::Accelerator,
            estimated_total_ns: accelerator_total,
            transfer_ns,
        })
    } else {
        Ok(AcceleratorPlan {
            target: ExecutionTarget::Cpu,
            estimated_total_ns: request.estimated_cpu_ns,
            transfer_ns: 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::hardware::{CacheProfile, HardwareProfile};

    fn hardware(accelerator: bool) -> HardwareProfile {
        let mut features = BTreeSet::new();
        if accelerator {
            features.insert(HardwareFeature::Accelerator);
        }
        HardwareProfile {
            id: "test".into(),
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
            features,
        }
    }

    fn request() -> AcceleratorRequest {
        AcceleratorRequest {
            work: AcceleratorWorkKind::ElementWise,
            elements: 10_000_000,
            input_bytes: 40_000_000,
            output_bytes: 40_000_000,
            estimated_cpu_ns: 100_000_000,
            estimated_accelerator_ns: 5_000_000,
            transfer_bandwidth_bytes_per_s: 16_000_000_000,
            fixed_transfer_latency_ns: 100_000,
            semantics: AcceleratorSemantics::Exact,
            secret_data: false,
            accelerator_secret_capable: false,
        }
    }

    #[test]
    fn offload_includes_transfer_cost() {
        let plan = plan_acceleration(&hardware(true), &request()).unwrap();
        assert_eq!(plan.target, ExecutionTarget::Accelerator);
        assert!(plan.transfer_ns > 0);
    }

    #[test]
    fn no_accelerator_falls_back_to_cpu_semantically() {
        let plan = plan_acceleration(&hardware(false), &request()).unwrap();
        assert_eq!(plan.target, ExecutionTarget::Cpu);
    }

    #[test]
    fn secret_data_requires_secret_capable_accelerator() {
        let mut request = request();
        request.secret_data = true;

        assert_eq!(
            plan_acceleration(&hardware(true), &request),
            Err(AcceleratorIssue::SecretDataNotAllowed)
        );
    }
}
