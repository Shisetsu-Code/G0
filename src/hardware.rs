use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HardwareFeature {
    Vector128,
    Vector256,
    Vector512,
    FusedMultiplyAdd,
    BitManipulation,
    PopulationCount,
    HardwareAes,
    HardwareRandom,
    Iommu,
    Numa,
    Accelerator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheProfile {
    pub line_bytes: u32,
    pub l1_data_bytes: u64,
    pub l2_bytes_per_core: u64,
    pub l3_bytes_total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareProfile {
    pub id: String,
    pub logical_cores: u32,
    pub physical_cores: u32,
    pub memory_bytes: u64,
    pub numa_nodes: u32,
    pub cache: CacheProfile,
    pub features: BTreeSet<HardwareFeature>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HardwareRequirement {
    pub min_logical_cores: u32,
    pub min_memory_bytes: u64,
    pub required_features: BTreeSet<HardwareFeature>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HardwareIssue {
    InsufficientLogicalCores {
        required: u32,
        available: u32,
    },
    InsufficientMemory {
        required: u64,
        available: u64,
    },
    MissingFeature(HardwareFeature),
    InvalidCacheLine,
    InvalidNumaCount,
}

pub fn validate_hardware_profile(
    profile: &HardwareProfile,
    requirement: &HardwareRequirement,
) -> Result<(), Vec<HardwareIssue>> {
    let mut issues = Vec::new();

    if profile.logical_cores < requirement.min_logical_cores {
        issues.push(HardwareIssue::InsufficientLogicalCores {
            required: requirement.min_logical_cores,
            available: profile.logical_cores,
        });
    }

    if profile.memory_bytes < requirement.min_memory_bytes {
        issues.push(HardwareIssue::InsufficientMemory {
            required: requirement.min_memory_bytes,
            available: profile.memory_bytes,
        });
    }

    if profile.cache.line_bytes == 0
        || !profile.cache.line_bytes.is_power_of_two()
    {
        issues.push(HardwareIssue::InvalidCacheLine);
    }

    if profile.numa_nodes == 0 {
        issues.push(HardwareIssue::InvalidNumaCount);
    }

    for feature in &requirement.required_features {
        if !profile.features.contains(feature) {
            issues.push(HardwareIssue::MissingFeature(*feature));
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn preferred_vector_bits(profile: &HardwareProfile) -> u16 {
    if profile.features.contains(&HardwareFeature::Vector512) {
        512
    } else if profile.features.contains(&HardwareFeature::Vector256) {
        256
    } else if profile.features.contains(&HardwareFeature::Vector128) {
        128
    } else {
        0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalityHint {
    CoreLocal,
    NumaLocal,
    SharedLastLevelCache,
    GlobalMemory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlacementHint {
    pub memory_region: String,
    pub locality: LocalityHint,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modern_cpu() -> HardwareProfile {
        HardwareProfile {
            id: "desktop".into(),
            logical_cores: 12,
            physical_cores: 6,
            memory_bytes: 32 * 1024 * 1024 * 1024,
            numa_nodes: 1,
            cache: CacheProfile {
                line_bytes: 64,
                l1_data_bytes: 32 * 1024,
                l2_bytes_per_core: 512 * 1024,
                l3_bytes_total: 32 * 1024 * 1024,
            },
            features: [
                HardwareFeature::Vector128,
                HardwareFeature::Vector256,
                HardwareFeature::FusedMultiplyAdd,
                HardwareFeature::BitManipulation,
                HardwareFeature::PopulationCount,
                HardwareFeature::Iommu,
            ]
            .into_iter()
            .collect(),
        }
    }

    #[test]
    fn hardware_requirement_has_no_legacy_fallback() {
        let requirement = HardwareRequirement {
            min_logical_cores: 8,
            min_memory_bytes: 16 * 1024 * 1024 * 1024,
            required_features: [HardwareFeature::Vector256]
                .into_iter()
                .collect(),
        };

        assert!(validate_hardware_profile(&modern_cpu(), &requirement).is_ok());
    }

    #[test]
    fn unsupported_feature_is_rejected_not_emulated_silently() {
        let requirement = HardwareRequirement {
            required_features: [HardwareFeature::Vector512]
                .into_iter()
                .collect(),
            ..HardwareRequirement::default()
        };

        assert_eq!(
            validate_hardware_profile(&modern_cpu(), &requirement),
            Err(vec![HardwareIssue::MissingFeature(
                HardwareFeature::Vector512
            )])
        );
    }

    #[test]
    fn vector_width_is_derived_from_profile() {
        assert_eq!(preferred_vector_bits(&modern_cpu()), 256);
    }
}
