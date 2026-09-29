use std::collections::BTreeSet;

use crate::gir::CapabilityClass;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Action(pub String);

impl Action {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResourceKind(pub String);

impl ResourceKind {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ScopeExpr {
    Global,
    CurrentPrincipal,
    CurrentScope,
    ResourceSelf,
    RelationPath(Vec<String>),
    Named(String),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct CapabilityGrant {
    pub class: CapabilityClass,
    pub action: Action,
    pub resource: ResourceKind,
    pub scope: ScopeExpr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Role {
    pub name: String,
    pub grants: BTreeSet<CapabilityGrant>,
}

impl Role {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            grants: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyExpr {
    Requires(CapabilityGrant),
    PrincipalOwnsResource,
    PrincipalInRelation { relation: String },
    All(Vec<PolicyExpr>),
    Any(Vec<PolicyExpr>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyRule {
    pub action: Action,
    pub allow_if: PolicyExpr,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PolicySet {
    /// Absence of a rule always means deny.
    pub rules: Vec<PolicyRule>,
}

impl PolicySet {
    pub fn rule_for(&self, action: &Action) -> Option<&PolicyRule> {
        self.rules.iter().find(|rule| &rule.action == action)
    }

    pub fn is_default_deny(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_policy_is_deny_by_construction() {
        let policies = PolicySet::default();
        assert!(policies.rule_for(&Action::new("read")).is_none());
        assert!(policies.is_default_deny());
    }

    #[test]
    fn roles_are_capability_bundles_not_authority_checks() {
        let mut role = Role::new("moderator");
        role.grants.insert(CapabilityGrant {
            class: CapabilityClass::Resource,
            action: Action::new("hide"),
            resource: ResourceKind::new("Message"),
            scope: ScopeExpr::CurrentScope,
        });
        assert_eq!(role.grants.len(), 1);
    }
}
