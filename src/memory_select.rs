use std::collections::BTreeMap;

use crate::hardware::{preferred_vector_bits, HardwareProfile};
use crate::memory::{
    validate_memory_plan, validate_physical_selection, AllocationIntent,
    LayoutKind, LifetimeClass, MemoryDomain, MemoryPlan, ParameterPolicy,
    PhysicalAllocation, PhysicalMemoryPlan, PhysicalSelectionIssue,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySelectionProfile {
    pub max_register_bytes: u64,
    pub max_stack_bytes: u64,
    pub prefer_regions: bool,
}

impl Default for MemorySelectionProfile {
    fn default() -> Self {
        Self {
            max_register_bytes: 16,
            max_stack_bytes: 64 * 1024,
            prefer_regions: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemorySelectionIssue {
    InvalidIntent,
    NoSelectableDomain(String),
    Selection(Vec<PhysicalSelectionIssue>),
}

pub fn select_memory_plan(
    plan: &MemoryPlan,
    hardware: &HardwareProfile,
    profile: MemorySelectionProfile,
) -> Result<PhysicalMemoryPlan, Vec<MemorySelectionIssue>> {
    if validate_memory_plan(plan).is_err() {
        return Err(vec![MemorySelectionIssue::InvalidIntent]);
    }

    let mut allocations = BTreeMap::new();
    let mut issues = Vec::new();

    for intent in &plan.allocations {
        let size = select_u64(intent.size_bytes);
        let alignment = select_u32(intent.alignment_bytes);
        let Some(domain) = choose_domain(intent, size, profile) else {
            issues.push(MemorySelectionIssue::NoSelectableDomain(
                intent.id.clone(),
            ));
            continue;
        };

        let layout = choose_layout(intent, hardware);

        allocations.insert(
            intent.id.clone(),
            PhysicalAllocation {
                id: intent.id.clone(),
                domain,
                size_bytes: size,
                alignment_bytes: alignment,
                layout,
            },
        );
    }

    if !issues.is_empty() {
        return Err(issues);
    }

    let selected = PhysicalMemoryPlan { allocations };
    if let Err(selection_issues) =
        validate_physical_selection(plan, &selected)
    {
        return Err(vec![MemorySelectionIssue::Selection(
            selection_issues,
        )]);
    }

    Ok(selected)
}

fn choose_domain(
    intent: &AllocationIntent,
    size: u64,
    profile: MemorySelectionProfile,
) -> Option<MemoryDomain> {
    if intent.zero_copy {
        for candidate in [
            MemoryDomain::Dma,
            MemoryDomain::Shared,
            MemoryDomain::Accelerator,
            MemoryDomain::Device,
        ] {
            if intent.allowed_domains.contains(&candidate) {
                return Some(candidate);
            }
        }
    }

    if intent.lifetime == LifetimeClass::NodeLocal
        && size <= profile.max_register_bytes
        && intent.allowed_domains.contains(&MemoryDomain::Register)
    {
        return Some(MemoryDomain::Register);
    }

    if matches!(
        intent.lifetime,
        LifetimeClass::NodeLocal | LifetimeClass::Graph
    ) && size <= profile.max_stack_bytes
        && intent.allowed_domains.contains(&MemoryDomain::Stack)
    {
        return Some(MemoryDomain::Stack);
    }

    if profile.prefer_regions
        && intent.allowed_domains.contains(&MemoryDomain::Region)
    {
        return Some(MemoryDomain::Region);
    }

    [
        MemoryDomain::Heap,
        MemoryDomain::Shared,
        MemoryDomain::Device,
        MemoryDomain::Accelerator,
        MemoryDomain::Dma,
        MemoryDomain::Region,
        MemoryDomain::Stack,
        MemoryDomain::Register,
    ]
    .into_iter()
    .find(|domain| intent.allowed_domains.contains(domain))
}

fn choose_layout(
    intent: &AllocationIntent,
    hardware: &HardwareProfile,
) -> LayoutKind {
    if preferred_vector_bits(hardware) != 0
        && intent.layout.contains(&LayoutKind::StructOfArrays)
    {
        return LayoutKind::StructOfArrays;
    }

    intent
        .layout
        .first()
        .copied()
        .unwrap_or(LayoutKind::Native)
}

fn select_u64(policy: ParameterPolicy<u64>) -> u64 {
    match policy {
        ParameterPolicy::Fixed(value) => value,
        ParameterPolicy::Bounded { min, .. } => min,
        ParameterPolicy::Free => 1,
    }
}

fn select_u32(policy: ParameterPolicy<u32>) -> u32 {
    match policy {
        ParameterPolicy::Fixed(value) => value,
        ParameterPolicy::Bounded { min, .. } => min,
        ParameterPolicy::Free => 1,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::hardware::{CacheProfile, HardwareFeature};

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
            features: BTreeSet::from([HardwareFeature::Vector256]),
        }
    }

    #[test]
    fn small_graph_allocation_uses_stack_when_allowed() {
        let plan = MemoryPlan {
            allocations: vec![AllocationIntent {
                id: "temp".into(),
                size_bytes: ParameterPolicy::Fixed(4096),
                alignment_bytes: ParameterPolicy::Fixed(64),
                lifetime: LifetimeClass::Graph,
                interval: None,
                allowed_domains: BTreeSet::from([
                    MemoryDomain::Stack,
                    MemoryDomain::Heap,
                ]),
                layout: vec![LayoutKind::Native],
                zero_copy: false,
                movable: true,
                pinned: false,
            }],
        };

        let selected = select_memory_plan(
            &plan,
            &hardware(),
            MemorySelectionProfile::default(),
        )
        .unwrap();

        assert_eq!(
            selected.allocations["temp"].domain,
            MemoryDomain::Stack
        );
    }

    #[test]
    fn vector_hardware_prefers_soa_when_allowed() {
        let plan = MemoryPlan {
            allocations: vec![AllocationIntent {
                id: "table".into(),
                size_bytes: ParameterPolicy::Fixed(1_000_000),
                alignment_bytes: ParameterPolicy::Fixed(64),
                lifetime: LifetimeClass::Region,
                interval: None,
                allowed_domains: BTreeSet::from([MemoryDomain::Region]),
                layout: vec![
                    LayoutKind::ArrayOfStructs,
                    LayoutKind::StructOfArrays,
                ],
                zero_copy: false,
                movable: true,
                pinned: false,
            }],
        };

        let selected = select_memory_plan(
            &plan,
            &hardware(),
            MemorySelectionProfile::default(),
        )
        .unwrap();

        assert_eq!(
            selected.allocations["table"].layout,
            LayoutKind::StructOfArrays
        );
    }
}
