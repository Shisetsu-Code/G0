use crate::authority::{
    Action, CapabilityGrant, PolicyExpr, PolicySet,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageGuard {
    Capability(CapabilityGrant),
    OwnerIsPrincipal,
    RelationContainsPrincipal(String),
    All(Vec<StorageGuard>),
    Any(Vec<StorageGuard>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledPolicy {
    pub action: Action,
    pub guard: StorageGuard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyCompileIssue {
    MissingAction(Action),
    EmptyAll,
    EmptyAny,
}

pub fn compile_policy(
    policies: &PolicySet,
    action: &Action,
) -> Result<CompiledPolicy, Vec<PolicyCompileIssue>> {
    let Some(rule) = policies.rule_for(action) else {
        return Err(vec![PolicyCompileIssue::MissingAction(
            action.clone(),
        )]);
    };

    let mut issues = Vec::new();
    let guard = lower_expr(&rule.allow_if, &mut issues);

    if issues.is_empty() {
        Ok(CompiledPolicy {
            action: action.clone(),
            guard,
        })
    } else {
        Err(issues)
    }
}

fn lower_expr(
    expr: &PolicyExpr,
    issues: &mut Vec<PolicyCompileIssue>,
) -> StorageGuard {
    match expr {
        PolicyExpr::Requires(capability) => {
            StorageGuard::Capability(capability.clone())
        }
        PolicyExpr::PrincipalOwnsResource => StorageGuard::OwnerIsPrincipal,
        PolicyExpr::PrincipalInRelation { relation } => {
            StorageGuard::RelationContainsPrincipal(relation.clone())
        }
        PolicyExpr::All(expressions) => {
            if expressions.is_empty() {
                issues.push(PolicyCompileIssue::EmptyAll);
            }
            StorageGuard::All(
                expressions
                    .iter()
                    .map(|expr| lower_expr(expr, issues))
                    .collect(),
            )
        }
        PolicyExpr::Any(expressions) => {
            if expressions.is_empty() {
                issues.push(PolicyCompileIssue::EmptyAny);
            }
            StorageGuard::Any(
                expressions
                    .iter()
                    .map(|expr| lower_expr(expr, issues))
                    .collect(),
            )
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnforcementLocation {
    StoreBeforeRead,
    StoreBeforeMutation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnforcementPlan {
    pub policy: CompiledPolicy,
    pub location: EnforcementLocation,
}

pub fn enforcement_plan(
    policies: &PolicySet,
    action: &Action,
) -> Result<EnforcementPlan, Vec<PolicyCompileIssue>> {
    let policy = compile_policy(policies, action)?;
    let location = if *action == Action::read()
        || *action == Action::enumerate()
    {
        EnforcementLocation::StoreBeforeRead
    } else {
        EnforcementLocation::StoreBeforeMutation
    };

    Ok(EnforcementPlan { policy, location })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::{PolicyRule, PolicySet};

    #[test]
    fn owner_policy_compiles_to_store_guard() {
        let policies = PolicySet {
            rules: vec![PolicyRule {
                action: Action::read(),
                allow_if: PolicyExpr::PrincipalOwnsResource,
            }],
        };

        let plan = enforcement_plan(&policies, &Action::read()).unwrap();
        assert_eq!(plan.policy.guard, StorageGuard::OwnerIsPrincipal);
        assert_eq!(
            plan.location,
            EnforcementLocation::StoreBeforeRead
        );
    }

    #[test]
    fn missing_policy_cannot_compile_to_allow_all() {
        assert_eq!(
            compile_policy(&PolicySet::default(), &Action::read()),
            Err(vec![PolicyCompileIssue::MissingAction(Action::read())])
        );
    }

    #[test]
    fn empty_boolean_policy_is_rejected() {
        let policies = PolicySet {
            rules: vec![PolicyRule {
                action: Action::read(),
                allow_if: PolicyExpr::All(vec![]),
            }],
        };

        assert_eq!(
            compile_policy(&policies, &Action::read()),
            Err(vec![PolicyCompileIssue::EmptyAll])
        );
    }
}
