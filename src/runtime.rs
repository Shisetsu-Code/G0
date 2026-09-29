use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionKind {
    Graph,
    Request,
    Task,
    Persistent,
    Secret,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionSpec {
    pub id: String,
    pub parent: Option<String>,
    pub kind: RegionKind,
    pub max_bytes: Option<u64>,
    pub zero_on_release: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionResource {
    pub id: String,
    pub region: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceReference {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RegionPlan {
    pub regions: Vec<RegionSpec>,
    pub resources: Vec<RegionResource>,
    pub references: Vec<ResourceReference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionIssue {
    DuplicateRegion(String),
    DuplicateResource(String),
    UnknownParent { region: String, parent: String },
    RegionCycle(String),
    SecretRegionMustZeroize(String),
    UnknownResourceRegion { resource: String, region: String },
    UnknownReferencedResource(String),
    LifetimeEscape { holder: String, target: String },
}

pub fn validate_region_plan(plan: &RegionPlan) -> Result<(), Vec<RegionIssue>> {
    let mut issues = Vec::new();
    let mut regions = BTreeMap::<&str, &RegionSpec>::new();

    for region in &plan.regions {
        if regions.insert(region.id.as_str(), region).is_some() {
            issues.push(RegionIssue::DuplicateRegion(region.id.clone()));
        }
        if region.kind == RegionKind::Secret && !region.zero_on_release {
            issues.push(RegionIssue::SecretRegionMustZeroize(region.id.clone()));
        }
    }

    for region in &plan.regions {
        if let Some(parent) = &region.parent
            && !regions.contains_key(parent.as_str())
        {
            issues.push(RegionIssue::UnknownParent {
                region: region.id.clone(),
                parent: parent.clone(),
            });
        }
    }

    for region in &plan.regions {
        if region_cycle(region.id.as_str(), &regions) {
            issues.push(RegionIssue::RegionCycle(region.id.clone()));
        }
    }

    let mut resources = BTreeMap::<&str, &RegionResource>::new();
    for resource in &plan.resources {
        if resources.insert(resource.id.as_str(), resource).is_some() {
            issues.push(RegionIssue::DuplicateResource(resource.id.clone()));
        }
        if !regions.contains_key(resource.region.as_str()) {
            issues.push(RegionIssue::UnknownResourceRegion {
                resource: resource.id.clone(),
                region: resource.region.clone(),
            });
        }
    }

    for reference in &plan.references {
        let Some(holder) = resources.get(reference.from.as_str()).copied() else {
            issues.push(RegionIssue::UnknownReferencedResource(reference.from.clone()));
            continue;
        };
        let Some(target) = resources.get(reference.to.as_str()).copied() else {
            issues.push(RegionIssue::UnknownReferencedResource(reference.to.clone()));
            continue;
        };

        if !region_outlives(
            target.region.as_str(),
            holder.region.as_str(),
            &regions,
        ) {
            issues.push(RegionIssue::LifetimeEscape {
                holder: reference.from.clone(),
                target: reference.to.clone(),
            });
        }
    }

    if issues.is_empty() { Ok(()) } else { Err(issues) }
}

fn region_outlives(
    candidate: &str,
    region: &str,
    regions: &BTreeMap<&str, &RegionSpec>,
) -> bool {
    if candidate == region {
        return true;
    }

    let mut current = region;
    let mut seen = BTreeSet::new();

    while seen.insert(current) {
        let Some(spec) = regions.get(current).copied() else {
            return false;
        };
        let Some(parent) = spec.parent.as_deref() else {
            return false;
        };
        if parent == candidate {
            return true;
        }
        current = parent;
    }

    false
}

fn region_cycle(start: &str, regions: &BTreeMap<&str, &RegionSpec>) -> bool {
    let mut current = start;
    let mut seen = BTreeSet::new();

    while seen.insert(current) {
        let Some(region) = regions.get(current).copied() else {
            return false;
        };
        let Some(parent) = region.parent.as_deref() else {
            return false;
        };
        current = parent;
    }

    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReleaseMode {
    DropRegion,
    ZeroAndDropRegion,
}

pub fn release_mode(region: &RegionSpec) -> ReleaseMode {
    if region.zero_on_release || region.kind == RegionKind::Secret {
        ReleaseMode::ZeroAndDropRegion
    } else {
        ReleaseMode::DropRegion
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_can_reference_parent_but_parent_cannot_reference_child() {
        let plan = RegionPlan {
            regions: vec![
                RegionSpec {
                    id: "app".into(),
                    parent: None,
                    kind: RegionKind::Persistent,
                    max_bytes: None,
                    zero_on_release: false,
                },
                RegionSpec {
                    id: "request".into(),
                    parent: Some("app".into()),
                    kind: RegionKind::Request,
                    max_bytes: Some(1024 * 1024),
                    zero_on_release: false,
                },
            ],
            resources: vec![
                RegionResource { id: "config".into(), region: "app".into() },
                RegionResource { id: "body".into(), region: "request".into() },
            ],
            references: vec![ResourceReference {
                from: "body".into(),
                to: "config".into(),
            }],
        };

        assert!(validate_region_plan(&plan).is_ok());
    }

    #[test]
    fn longer_lived_resource_cannot_hold_short_lived_reference() {
        let plan = RegionPlan {
            regions: vec![
                RegionSpec {
                    id: "app".into(),
                    parent: None,
                    kind: RegionKind::Persistent,
                    max_bytes: None,
                    zero_on_release: false,
                },
                RegionSpec {
                    id: "request".into(),
                    parent: Some("app".into()),
                    kind: RegionKind::Request,
                    max_bytes: None,
                    zero_on_release: false,
                },
            ],
            resources: vec![
                RegionResource { id: "cache".into(), region: "app".into() },
                RegionResource { id: "temporary".into(), region: "request".into() },
            ],
            references: vec![ResourceReference {
                from: "cache".into(),
                to: "temporary".into(),
            }],
        };

        assert!(validate_region_plan(&plan)
            .unwrap_err()
            .contains(&RegionIssue::LifetimeEscape {
                holder: "cache".into(),
                target: "temporary".into(),
            }));
    }

    #[test]
    fn secret_region_zeroization_is_mandatory() {
        let plan = RegionPlan {
            regions: vec![RegionSpec {
                id: "secrets".into(),
                parent: None,
                kind: RegionKind::Secret,
                max_bytes: Some(4096),
                zero_on_release: false,
            }],
            ..RegionPlan::default()
        };

        assert!(validate_region_plan(&plan)
            .unwrap_err()
            .contains(&RegionIssue::SecretRegionMustZeroize("secrets".into())));
    }
}
