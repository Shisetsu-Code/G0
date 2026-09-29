use std::collections::{BTreeMap, BTreeSet};

use crate::gir::{IntegerType, NodeId, ParameterPolicy};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IntegerWidth {
    U8,
    U16,
    U32,
    U64,
    U128,
    I8,
    I16,
    I32,
    I64,
    I128,
}

impl IntegerWidth {
    pub fn bits(self) -> u16 {
        match self {
            Self::U8 | Self::I8 => 8,
            Self::U16 | Self::I16 => 16,
            Self::U32 | Self::I32 => 32,
            Self::U64 | Self::I64 => 64,
            Self::U128 | Self::I128 => 128,
        }
    }
}

pub fn choose_integer_width(range: &IntegerType) -> IntegerWidth {
    if range.min >= 0 {
        let max = range.max as u128;
        if max <= u8::MAX as u128 {
            IntegerWidth::U8
        } else if max <= u16::MAX as u128 {
            IntegerWidth::U16
        } else if max <= u32::MAX as u128 {
            IntegerWidth::U32
        } else if max <= u64::MAX as u128 {
            IntegerWidth::U64
        } else {
            IntegerWidth::U128
        }
    } else if range.min >= i8::MIN as i128 && range.max <= i8::MAX as i128 {
        IntegerWidth::I8
    } else if range.min >= i16::MIN as i128 && range.max <= i16::MAX as i128 {
        IntegerWidth::I16
    } else if range.min >= i32::MIN as i128 && range.max <= i32::MAX as i128 {
        IntegerWidth::I32
    } else if range.min >= i64::MIN as i128 && range.max <= i64::MAX as i128 {
        IntegerWidth::I64
    } else {
        IntegerWidth::I128
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MemoryDomain {
    Register,
    Stack,
    Region,
    Heap,
    Shared,
    Device,
    Accelerator,
    Dma,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifetimeClass {
    NodeLocal,
    Graph,
    Region,
    Persistent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutKind {
    Native,
    Packed,
    ArrayOfStructs,
    StructOfArrays,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifetimeInterval {
    pub first_use: NodeId,
    pub last_use: NodeId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AllocationIntent {
    pub id: String,
    pub size_bytes: ParameterPolicy<u64>,
    pub alignment_bytes: ParameterPolicy<u32>,
    pub lifetime: LifetimeClass,
    pub interval: Option<LifetimeInterval>,
    pub allowed_domains: BTreeSet<MemoryDomain>,
    pub layout: Vec<LayoutKind>,
    pub zero_copy: bool,
    pub movable: bool,
    pub pinned: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MemoryPlan {
    pub allocations: Vec<AllocationIntent>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryIssue {
    DuplicateAllocation(String),
    NoAllowedDomain(String),
    InvalidSizeBounds(String),
    InvalidAlignmentBounds(String),
    ZeroCopyWithoutSharedDomain(String),
    DmaRequiresPinned(String),
    PinnedCannotBeMovable(String),
    PersistentCannotUseRegisterOrStack(String),
}

pub fn validate_memory_plan(plan: &MemoryPlan) -> Result<(), Vec<MemoryIssue>> {
    let mut issues = Vec::new();
    let mut ids = BTreeSet::new();

    for allocation in &plan.allocations {
        if !ids.insert(allocation.id.as_str()) {
            issues.push(MemoryIssue::DuplicateAllocation(allocation.id.clone()));
        }

        if allocation.allowed_domains.is_empty() {
            issues.push(MemoryIssue::NoAllowedDomain(allocation.id.clone()));
        }

        if !valid_u64_policy(allocation.size_bytes) {
            issues.push(MemoryIssue::InvalidSizeBounds(allocation.id.clone()));
        }

        if !valid_u32_policy(allocation.alignment_bytes) {
            issues.push(MemoryIssue::InvalidAlignmentBounds(
                allocation.id.clone(),
            ));
        }

        if allocation.zero_copy
            && !allocation.allowed_domains.iter().any(|domain| {
                matches!(
                    domain,
                    MemoryDomain::Shared
                        | MemoryDomain::Dma
                        | MemoryDomain::Device
                        | MemoryDomain::Accelerator
                )
            })
        {
            issues.push(MemoryIssue::ZeroCopyWithoutSharedDomain(
                allocation.id.clone(),
            ));
        }

        if allocation.allowed_domains.contains(&MemoryDomain::Dma)
            && !allocation.pinned
        {
            issues.push(MemoryIssue::DmaRequiresPinned(
                allocation.id.clone(),
            ));
        }

        if allocation.pinned && allocation.movable {
            issues.push(MemoryIssue::PinnedCannotBeMovable(
                allocation.id.clone(),
            ));
        }

        if allocation.lifetime == LifetimeClass::Persistent
            && allocation.allowed_domains.iter().any(|domain| {
                matches!(domain, MemoryDomain::Register | MemoryDomain::Stack)
            })
        {
            issues.push(MemoryIssue::PersistentCannotUseRegisterOrStack(
                allocation.id.clone(),
            ));
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn valid_u64_policy(policy: ParameterPolicy<u64>) -> bool {
    match policy {
        ParameterPolicy::Fixed(value) => value > 0,
        ParameterPolicy::Bounded { min, max } => min > 0 && min <= max,
        ParameterPolicy::Free => true,
    }
}

fn valid_u32_policy(policy: ParameterPolicy<u32>) -> bool {
    match policy {
        ParameterPolicy::Fixed(value) => value.is_power_of_two(),
        ParameterPolicy::Bounded { min, max } => {
            min > 0
                && min <= max
                && min.is_power_of_two()
                && max.is_power_of_two()
        }
        ParameterPolicy::Free => true,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalAllocation {
    pub id: String,
    pub domain: MemoryDomain,
    pub size_bytes: u64,
    pub alignment_bytes: u32,
    pub layout: LayoutKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PhysicalMemoryPlan {
    pub allocations: BTreeMap<String, PhysicalAllocation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhysicalSelectionIssue {
    UnknownAllocation(String),
    DomainNotAllowed {
        allocation: String,
        domain: MemoryDomain,
    },
    SizeOutsidePolicy(String),
    AlignmentOutsidePolicy(String),
    LayoutNotAllowed(String),
}

pub fn validate_physical_selection(
    intent: &MemoryPlan,
    selected: &PhysicalMemoryPlan,
) -> Result<(), Vec<PhysicalSelectionIssue>> {
    let mut issues = Vec::new();
    let intents: BTreeMap<&str, &AllocationIntent> = intent
        .allocations
        .iter()
        .map(|allocation| (allocation.id.as_str(), allocation))
        .collect();

    for (id, physical) in &selected.allocations {
        let Some(expected) = intents.get(id.as_str()).copied() else {
            issues.push(PhysicalSelectionIssue::UnknownAllocation(id.clone()));
            continue;
        };

        if !expected.allowed_domains.contains(&physical.domain) {
            issues.push(PhysicalSelectionIssue::DomainNotAllowed {
                allocation: id.clone(),
                domain: physical.domain,
            });
        }
        if !u64_policy_contains(expected.size_bytes, physical.size_bytes) {
            issues.push(PhysicalSelectionIssue::SizeOutsidePolicy(id.clone()));
        }
        if !u32_policy_contains(
            expected.alignment_bytes,
            physical.alignment_bytes,
        ) {
            issues.push(PhysicalSelectionIssue::AlignmentOutsidePolicy(
                id.clone(),
            ));
        }
        if !expected.layout.is_empty()
            && !expected.layout.contains(&physical.layout)
        {
            issues.push(PhysicalSelectionIssue::LayoutNotAllowed(id.clone()));
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn u64_policy_contains(policy: ParameterPolicy<u64>, value: u64) -> bool {
    match policy {
        ParameterPolicy::Fixed(required) => value == required,
        ParameterPolicy::Bounded { min, max } => value >= min && value <= max,
        ParameterPolicy::Free => true,
    }
}

fn u32_policy_contains(policy: ParameterPolicy<u32>, value: u32) -> bool {
    match policy {
        ParameterPolicy::Fixed(required) => value == required,
        ParameterPolicy::Bounded { min, max } => value >= min && value <= max,
        ParameterPolicy::Free => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_range_selects_minimum_physical_width() {
        assert_eq!(
            choose_integer_width(&IntegerType::new(0, 150).unwrap()),
            IntegerWidth::U8
        );
        assert_eq!(
            choose_integer_width(&IntegerType::new(0, 1_000_000).unwrap()),
            IntegerWidth::U32
        );
        assert_eq!(
            choose_integer_width(&IntegerType::new(-20, 20).unwrap()),
            IntegerWidth::I8
        );
    }

    #[test]
    fn zero_copy_requires_a_shareable_memory_domain() {
        let plan = MemoryPlan {
            allocations: vec![AllocationIntent {
                id: "buffer".into(),
                size_bytes: ParameterPolicy::Fixed(4096),
                alignment_bytes: ParameterPolicy::Fixed(64),
                lifetime: LifetimeClass::Graph,
                interval: None,
                allowed_domains: [MemoryDomain::Stack].into_iter().collect(),
                layout: vec![LayoutKind::Native],
                zero_copy: true,
                movable: true,
                pinned: false,
            }],
        };

        assert!(validate_memory_plan(&plan)
            .unwrap_err()
            .contains(&MemoryIssue::ZeroCopyWithoutSharedDomain(
                "buffer".into()
            )));
    }

    #[test]
    fn dma_memory_is_pinned_by_contract() {
        let plan = MemoryPlan {
            allocations: vec![AllocationIntent {
                id: "rx".into(),
                size_bytes: ParameterPolicy::Fixed(65_536),
                alignment_bytes: ParameterPolicy::Fixed(4096),
                lifetime: LifetimeClass::Region,
                interval: None,
                allowed_domains: [MemoryDomain::Dma].into_iter().collect(),
                layout: vec![LayoutKind::Native],
                zero_copy: true,
                movable: false,
                pinned: true,
            }],
        };

        assert!(validate_memory_plan(&plan).is_ok());
    }

    #[test]
    fn physical_selection_must_stay_inside_semantic_intent() {
        let intent = MemoryPlan {
            allocations: vec![AllocationIntent {
                id: "cache".into(),
                size_bytes: ParameterPolicy::Bounded {
                    min: 1024,
                    max: 65_536,
                },
                alignment_bytes: ParameterPolicy::Fixed(64),
                lifetime: LifetimeClass::Graph,
                interval: None,
                allowed_domains: [MemoryDomain::Region, MemoryDomain::Heap]
                    .into_iter()
                    .collect(),
                layout: vec![
                    LayoutKind::ArrayOfStructs,
                    LayoutKind::StructOfArrays,
                ],
                zero_copy: false,
                movable: true,
                pinned: false,
            }],
        };

        let selected = PhysicalMemoryPlan {
            allocations: BTreeMap::from([(
                "cache".into(),
                PhysicalAllocation {
                    id: "cache".into(),
                    domain: MemoryDomain::Region,
                    size_bytes: 16_384,
                    alignment_bytes: 64,
                    layout: LayoutKind::StructOfArrays,
                },
            )]),
        };

        assert!(validate_physical_selection(&intent, &selected).is_ok());
    }
}
