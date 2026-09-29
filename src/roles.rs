use std::collections::{BTreeMap, BTreeSet};

use crate::authority::CapabilityGrant;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleDefinition {
    pub name: String,
    pub grants: BTreeSet<CapabilityGrant>,
    pub includes: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoleRegistry {
    pub roles: BTreeMap<String, RoleDefinition>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoleIssue {
    NameMismatch {
        key: String,
        declared: String,
    },
    UnknownIncludedRole {
        role: String,
        included: String,
    },
    InclusionCycle(Vec<String>),
}

pub fn validate_role_registry(
    registry: &RoleRegistry,
) -> Result<(), Vec<RoleIssue>> {
    let mut issues = Vec::new();

    for (name, role) in &registry.roles {
        if name != &role.name {
            issues.push(RoleIssue::NameMismatch {
                key: name.clone(),
                declared: role.name.clone(),
            });
        }
        for included in &role.includes {
            if !registry.roles.contains_key(included) {
                issues.push(RoleIssue::UnknownIncludedRole {
                    role: name.clone(),
                    included: included.clone(),
                });
            }
        }
    }

    if issues.is_empty()
        && let Some(cycle) = find_cycle(registry)
    {
        issues.push(RoleIssue::InclusionCycle(cycle));
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn expand_role(
    registry: &RoleRegistry,
    role: &str,
) -> Option<BTreeSet<CapabilityGrant>> {
    if validate_role_registry(registry).is_err() {
        return None;
    }

    let root = registry.roles.get(role)?;
    let mut grants = root.grants.clone();
    let mut stack: Vec<String> = root.includes.iter().cloned().collect();
    let mut visited = BTreeSet::new();

    while let Some(name) = stack.pop() {
        if !visited.insert(name.clone()) {
            continue;
        }
        let included = registry.roles.get(&name)?;
        grants.extend(included.grants.iter().cloned());
        stack.extend(included.includes.iter().cloned());
    }

    Some(grants)
}

fn find_cycle(registry: &RoleRegistry) -> Option<Vec<String>> {
    fn visit(
        name: &str,
        registry: &RoleRegistry,
        visiting: &mut BTreeSet<String>,
        visited: &mut BTreeSet<String>,
        path: &mut Vec<String>,
    ) -> Option<Vec<String>> {
        if visiting.contains(name) {
            let start = path.iter().position(|item| item == name).unwrap_or(0);
            let mut cycle = path[start..].to_vec();
            cycle.push(name.to_owned());
            return Some(cycle);
        }
        if visited.contains(name) {
            return None;
        }

        visiting.insert(name.to_owned());
        path.push(name.to_owned());

        for included in &registry.roles.get(name)?.includes {
            if let Some(cycle) =
                visit(included, registry, visiting, visited, path)
            {
                return Some(cycle);
            }
        }

        path.pop();
        visiting.remove(name);
        visited.insert(name.to_owned());
        None
    }

    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut path = Vec::new();

    for name in registry.roles.keys() {
        if let Some(cycle) = visit(
            name,
            registry,
            &mut visiting,
            &mut visited,
            &mut path,
        ) {
            return Some(cycle);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::{
        Action, CapabilityGrant, ResourceKind, ScopeExpr,
    };
    use crate::gir::CapabilityClass;

    fn read_messages() -> CapabilityGrant {
        CapabilityGrant {
            class: CapabilityClass::Resource,
            action: Action::read(),
            resource: ResourceKind::new("Message"),
            scope: ScopeExpr::CurrentScope,
        }
    }

    #[test]
    fn role_inheritance_expands_to_capabilities() {
        let registry = RoleRegistry {
            roles: BTreeMap::from([
                (
                    "reader".into(),
                    RoleDefinition {
                        name: "reader".into(),
                        grants: BTreeSet::from([read_messages()]),
                        includes: BTreeSet::new(),
                    },
                ),
                (
                    "moderator".into(),
                    RoleDefinition {
                        name: "moderator".into(),
                        grants: BTreeSet::new(),
                        includes: BTreeSet::from(["reader".into()]),
                    },
                ),
            ]),
        };

        assert_eq!(
            expand_role(&registry, "moderator").unwrap(),
            BTreeSet::from([read_messages()])
        );
    }

    #[test]
    fn role_cycle_is_invalid() {
        let registry = RoleRegistry {
            roles: BTreeMap::from([
                (
                    "a".into(),
                    RoleDefinition {
                        name: "a".into(),
                        grants: BTreeSet::new(),
                        includes: BTreeSet::from(["b".into()]),
                    },
                ),
                (
                    "b".into(),
                    RoleDefinition {
                        name: "b".into(),
                        grants: BTreeSet::new(),
                        includes: BTreeSet::from(["a".into()]),
                    },
                ),
            ]),
        };

        assert!(matches!(
            validate_role_registry(&registry),
            Err(issues) if matches!(
                issues.first(),
                Some(RoleIssue::InclusionCycle(_))
            )
        ));
    }
}
