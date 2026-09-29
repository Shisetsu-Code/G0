use crate::authority::PolicySet;
use crate::authority::{
    Action, AuthorizationDecision, DenyReason, Principal, PrincipalId, ResourceContext, ResourceId,
    ResourceKind, ScopeId, authorize,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecurityEpoch {
    pub resource_version: u64,
    pub relation_version: u64,
    pub policy_version: u64,
}

impl SecurityEpoch {
    pub fn new(resource_version: u64, relation_version: u64, policy_version: u64) -> Self {
        Self {
            resource_version,
            relation_version,
            policy_version,
        }
    }
}

/// A mutation proof is intentionally non-Clone and is consumed when a mutation is authorized.
/// G0 lowering will model this as a unique value.
#[derive(Debug, PartialEq, Eq)]
pub struct MutationAuthorizationProof {
    principal: PrincipalId,
    resource_kind: ResourceKind,
    resource_id: ResourceId,
    scope: ScopeId,
    action: Action,
    epoch: SecurityEpoch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FreshnessIssue {
    AuthorizationDenied(DenyReason),
    PrincipalChanged,
    ResourceChanged,
    ScopeChanged,
    ActionChanged,
    SecurityStateChanged {
        authorized_at: SecurityEpoch,
        current: SecurityEpoch,
    },
}

pub fn issue_mutation_proof(
    principal: &Principal,
    resource: &ResourceContext,
    policies: &PolicySet,
    action: &Action,
    resource_is_global: bool,
    epoch: SecurityEpoch,
) -> Result<MutationAuthorizationProof, FreshnessIssue> {
    match authorize(principal, resource, policies, action, resource_is_global) {
        AuthorizationDecision::Allow => Ok(MutationAuthorizationProof {
            principal: principal.id.clone(),
            resource_kind: resource.kind.clone(),
            resource_id: resource.id.clone(),
            scope: resource.scope.clone(),
            action: action.clone(),
            epoch,
        }),
        AuthorizationDecision::Deny(reason) => Err(FreshnessIssue::AuthorizationDenied(reason)),
    }
}

pub fn consume_mutation_proof(
    proof: MutationAuthorizationProof,
    principal: &Principal,
    resource: &ResourceContext,
    action: &Action,
    current_epoch: SecurityEpoch,
) -> Result<(), FreshnessIssue> {
    if proof.principal != principal.id {
        return Err(FreshnessIssue::PrincipalChanged);
    }
    if proof.resource_kind != resource.kind || proof.resource_id != resource.id {
        return Err(FreshnessIssue::ResourceChanged);
    }
    if proof.scope != resource.scope {
        return Err(FreshnessIssue::ScopeChanged);
    }
    if proof.action != *action {
        return Err(FreshnessIssue::ActionChanged);
    }
    if proof.epoch != current_epoch {
        return Err(FreshnessIssue::SecurityStateChanged {
            authorized_at: proof.epoch,
            current: current_epoch,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::{PolicyExpr, PolicyRule};

    fn owner_update_policy() -> PolicySet {
        PolicySet {
            rules: vec![PolicyRule {
                action: Action::update(),
                allow_if: PolicyExpr::PrincipalOwnsResource,
            }],
        }
    }

    #[test]
    fn fresh_authorization_proof_can_be_consumed_for_same_mutation() {
        let principal = Principal::new("alice", "tenant-a");
        let mut resource = ResourceContext::new("Message", "m1", "tenant-a");
        resource.owner = Some(principal.id.clone());
        let epoch = SecurityEpoch::new(7, 3, 11);

        let proof = issue_mutation_proof(
            &principal,
            &resource,
            &owner_update_policy(),
            &Action::update(),
            false,
            epoch,
        )
        .unwrap();

        assert!(
            consume_mutation_proof(proof, &principal, &resource, &Action::update(), epoch,).is_ok()
        );
    }

    #[test]
    fn relation_or_policy_change_invalidates_prior_authorization() {
        let principal = Principal::new("alice", "tenant-a");
        let mut resource = ResourceContext::new("Message", "m1", "tenant-a");
        resource.owner = Some(principal.id.clone());

        let authorized_at = SecurityEpoch::new(7, 3, 11);
        let proof = issue_mutation_proof(
            &principal,
            &resource,
            &owner_update_policy(),
            &Action::update(),
            false,
            authorized_at,
        )
        .unwrap();

        let current = SecurityEpoch::new(7, 4, 11);
        assert_eq!(
            consume_mutation_proof(proof, &principal, &resource, &Action::update(), current,),
            Err(FreshnessIssue::SecurityStateChanged {
                authorized_at,
                current,
            })
        );
    }

    #[test]
    fn proof_for_one_resource_cannot_authorize_another_resource() {
        let principal = Principal::new("alice", "tenant-a");
        let mut first = ResourceContext::new("Message", "m1", "tenant-a");
        first.owner = Some(principal.id.clone());
        let mut second = ResourceContext::new("Message", "m2", "tenant-a");
        second.owner = Some(principal.id.clone());
        let epoch = SecurityEpoch::new(1, 1, 1);

        let proof = issue_mutation_proof(
            &principal,
            &first,
            &owner_update_policy(),
            &Action::update(),
            false,
            epoch,
        )
        .unwrap();

        assert_eq!(
            consume_mutation_proof(proof, &principal, &second, &Action::update(), epoch,),
            Err(FreshnessIssue::ResourceChanged)
        );
    }

    #[test]
    fn proof_for_read_cannot_be_reused_as_update_authority() {
        let principal = Principal::new("alice", "tenant-a");
        let mut resource = ResourceContext::new("Message", "m1", "tenant-a");
        resource.owner = Some(principal.id.clone());
        let policies = PolicySet {
            rules: vec![PolicyRule {
                action: Action::read(),
                allow_if: PolicyExpr::PrincipalOwnsResource,
            }],
        };
        let epoch = SecurityEpoch::new(1, 1, 1);

        let proof = issue_mutation_proof(
            &principal,
            &resource,
            &policies,
            &Action::read(),
            false,
            epoch,
        )
        .unwrap();

        assert_eq!(
            consume_mutation_proof(proof, &principal, &resource, &Action::update(), epoch,),
            Err(FreshnessIssue::ActionChanged)
        );
    }
}
