use std::collections::{BTreeMap, BTreeSet};

use crate::gir::CapabilityClass;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PrincipalId(pub String);

impl PrincipalId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScopeId(pub String);

impl ScopeId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResourceId(pub String);

impl ResourceId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Action(pub String);

impl Action {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn read() -> Self {
        Self::new("read")
    }

    pub fn create() -> Self {
        Self::new("create")
    }

    pub fn update() -> Self {
        Self::new("update")
    }

    pub fn delete() -> Self {
        Self::new("delete")
    }

    pub fn enumerate() -> Self {
        Self::new("enumerate")
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub id: PrincipalId,
    pub scope: ScopeId,
    pub grants: BTreeSet<CapabilityGrant>,
}

impl Principal {
    pub fn new(id: impl Into<String>, scope: impl Into<String>) -> Self {
        Self {
            id: PrincipalId::new(id),
            scope: ScopeId::new(scope),
            grants: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceContext {
    pub kind: ResourceKind,
    pub id: ResourceId,
    pub scope: ScopeId,
    pub owner: Option<PrincipalId>,
    /// Relation name -> principals related to this resource.
    pub principal_relations: BTreeMap<String, BTreeSet<PrincipalId>>,
}

impl ResourceContext {
    pub fn new(
        kind: impl Into<String>,
        id: impl Into<String>,
        scope: impl Into<String>,
    ) -> Self {
        Self {
            kind: ResourceKind::new(kind),
            id: ResourceId::new(id),
            scope: ScopeId::new(scope),
            owner: None,
            principal_relations: BTreeMap::new(),
        }
    }

    pub fn add_principal_relation(
        &mut self,
        relation: impl Into<String>,
        principal: PrincipalId,
    ) {
        self.principal_relations
            .entry(relation.into())
            .or_default()
            .insert(principal);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DenyReason {
    NoPolicy,
    ScopeMismatch,
    PolicyUnsatisfied,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorizationDecision {
    Allow,
    Deny(DenyReason),
}

impl AuthorizationDecision {
    pub fn allowed(&self) -> bool {
        matches!(self, Self::Allow)
    }
}

pub fn authorize(
    principal: &Principal,
    resource: &ResourceContext,
    policies: &PolicySet,
    action: &Action,
    resource_is_global: bool,
) -> AuthorizationDecision {
    if !resource_is_global && principal.scope != resource.scope {
        return AuthorizationDecision::Deny(DenyReason::ScopeMismatch);
    }

    let Some(rule) = policies.rule_for(action) else {
        return AuthorizationDecision::Deny(DenyReason::NoPolicy);
    };

    if evaluate_policy(&rule.allow_if, principal, resource) {
        AuthorizationDecision::Allow
    } else {
        AuthorizationDecision::Deny(DenyReason::PolicyUnsatisfied)
    }
}

fn evaluate_policy(expr: &PolicyExpr, principal: &Principal, resource: &ResourceContext) -> bool {
    match expr {
        PolicyExpr::Requires(required) => principal
            .grants
            .iter()
            .any(|grant| grant_satisfies(grant, required, principal, resource)),
        PolicyExpr::PrincipalOwnsResource => {
            resource.owner.as_ref().is_some_and(|owner| owner == &principal.id)
        }
        PolicyExpr::PrincipalInRelation { relation } => resource
            .principal_relations
            .get(relation)
            .is_some_and(|principals| principals.contains(&principal.id)),
        PolicyExpr::All(expressions) => expressions
            .iter()
            .all(|value| evaluate_policy(value, principal, resource)),
        PolicyExpr::Any(expressions) => expressions
            .iter()
            .any(|value| evaluate_policy(value, principal, resource)),
    }
}

fn grant_satisfies(
    actual: &CapabilityGrant,
    required: &CapabilityGrant,
    principal: &Principal,
    resource: &ResourceContext,
) -> bool {
    actual.class == required.class
        && actual.action == required.action
        && actual.resource == required.resource
        && scope_expr_matches(&actual.scope, principal, resource)
        && scope_requirement_compatible(&required.scope, principal, resource)
}

fn scope_requirement_compatible(
    required: &ScopeExpr,
    principal: &Principal,
    resource: &ResourceContext,
) -> bool {
    match required {
        ScopeExpr::Global => true,
        ScopeExpr::CurrentPrincipal => resource.owner.as_ref() == Some(&principal.id),
        ScopeExpr::CurrentScope => principal.scope == resource.scope,
        ScopeExpr::ResourceSelf => true,
        ScopeExpr::Named(scope) => resource.scope.0 == *scope,
        ScopeExpr::RelationPath(path) => relation_path_matches(path, principal, resource),
    }
}

fn scope_expr_matches(
    actual: &ScopeExpr,
    principal: &Principal,
    resource: &ResourceContext,
) -> bool {
    match actual {
        ScopeExpr::Global => true,
        ScopeExpr::CurrentPrincipal => resource.owner.as_ref() == Some(&principal.id),
        ScopeExpr::CurrentScope => principal.scope == resource.scope,
        ScopeExpr::ResourceSelf => true,
        ScopeExpr::Named(scope) => resource.scope.0 == *scope,
        ScopeExpr::RelationPath(path) => relation_path_matches(path, principal, resource),
    }
}

fn relation_path_matches(
    path: &[String],
    principal: &Principal,
    resource: &ResourceContext,
) -> bool {
    match path {
        [relation] => resource
            .principal_relations
            .get(relation)
            .is_some_and(|principals| principals.contains(&principal.id)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_message(scope: ScopeExpr) -> CapabilityGrant {
        CapabilityGrant {
            class: CapabilityClass::Resource,
            action: Action::read(),
            resource: ResourceKind::new("Message"),
            scope,
        }
    }

    #[test]
    fn absent_policy_is_deny_by_construction() {
        let policies = PolicySet::default();
        assert!(policies.rule_for(&Action::read()).is_none());
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

    #[test]
    fn owner_policy_allows_only_owner() {
        let policies = PolicySet {
            rules: vec![PolicyRule {
                action: Action::read(),
                allow_if: PolicyExpr::PrincipalOwnsResource,
            }],
        };

        let alice = Principal::new("alice", "tenant-a");
        let bob = Principal::new("bob", "tenant-a");
        let mut message = ResourceContext::new("Message", "m1", "tenant-a");
        message.owner = Some(alice.id.clone());

        assert!(authorize(&alice, &message, &policies, &Action::read(), false).allowed());
        assert!(!authorize(&bob, &message, &policies, &Action::read(), false).allowed());
    }

    #[test]
    fn relation_policy_allows_membership_without_ui_assumptions() {
        let policies = PolicySet {
            rules: vec![PolicyRule {
                action: Action::read(),
                allow_if: PolicyExpr::PrincipalInRelation {
                    relation: "conversation.member".into(),
                },
            }],
        };

        let alice = Principal::new("alice", "tenant-a");
        let bob = Principal::new("bob", "tenant-a");
        let mut message = ResourceContext::new("Message", "m1", "tenant-a");
        message.add_principal_relation("conversation.member", alice.id.clone());

        assert!(authorize(&alice, &message, &policies, &Action::read(), false).allowed());
        assert!(!authorize(&bob, &message, &policies, &Action::read(), false).allowed());
    }

    #[test]
    fn scoped_capability_cannot_cross_tenant() {
        let mut principal = Principal::new("alice", "tenant-a");
        principal.grants.insert(read_message(ScopeExpr::CurrentScope));

        let policies = PolicySet {
            rules: vec![PolicyRule {
                action: Action::read(),
                allow_if: PolicyExpr::Requires(read_message(ScopeExpr::CurrentScope)),
            }],
        };

        let other_tenant = ResourceContext::new("Message", "m2", "tenant-b");
        let decision = authorize(
            &principal,
            &other_tenant,
            &policies,
            &Action::read(),
            false,
        );

        assert_eq!(
            decision,
            AuthorizationDecision::Deny(DenyReason::ScopeMismatch)
        );
    }

    #[test]
    fn no_policy_denies_even_with_a_matching_capability() {
        let mut principal = Principal::new("alice", "tenant-a");
        principal.grants.insert(read_message(ScopeExpr::CurrentScope));

        let message = ResourceContext::new("Message", "m1", "tenant-a");
        let decision = authorize(
            &principal,
            &message,
            &PolicySet::default(),
            &Action::read(),
            false,
        );

        assert_eq!(decision, AuthorizationDecision::Deny(DenyReason::NoPolicy));
    }
}
