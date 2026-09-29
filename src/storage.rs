use std::collections::BTreeSet;

use crate::authority::{
    Action, AuthorizationDecision, DenyReason, PolicyExpr, PolicyRule, PolicySet, Principal,
    ResourceContext, authorize,
};
use crate::freshness::{
    FreshnessIssue, MutationAuthorizationProof, SecurityEpoch, consume_mutation_proof,
    issue_mutation_proof,
};
use crate::gir::SemanticType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldProtection {
    /// Protected by the store/platform at-rest profile, but not treated as application-sensitive.
    Public,
    /// Field-level protection and authorization are required.
    Private,
    /// Field-level protection plus secret-handling restrictions are required.
    Secret,
    /// Non-recoverable verifier semantics; the original value is never readable from storage.
    Credential,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncryptionRequirement {
    StoreAtRest,
    FieldProtected,
    SecretField,
    VerifierOnly,
}

impl FieldProtection {
    pub fn encryption_requirement(self) -> EncryptionRequirement {
        match self {
            Self::Public => EncryptionRequirement::StoreAtRest,
            Self::Private => EncryptionRequirement::FieldProtected,
            Self::Secret => EncryptionRequirement::SecretField,
            Self::Credential => EncryptionRequirement::VerifierOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TenantIsolation {
    /// Bound to the current application/security scope. This is the default.
    CurrentScope,
    /// Explicit global resource. Requires deliberate opt-in.
    Global,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cardinality {
    One,
    OptionalOne,
    Many,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeleteRule {
    Restrict,
    Cascade,
    Detach,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSchema {
    pub name: String,
    pub ty: SemanticType,
    pub protection: FieldProtection,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldAccessRule {
    Inherit,
    Deny,
    Require(PolicyExpr),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldPolicy {
    pub field: String,
    pub read: FieldAccessRule,
    pub create: FieldAccessRule,
    pub update: FieldAccessRule,
}

impl FieldPolicy {
    pub fn inherit(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            read: FieldAccessRule::Inherit,
            create: FieldAccessRule::Inherit,
            update: FieldAccessRule::Inherit,
        }
    }

    fn rule_for(&self, action: &Action) -> Option<&FieldAccessRule> {
        if action == &Action::read() {
            Some(&self.read)
        } else if action == &Action::create() {
            Some(&self.create)
        } else if action == &Action::update() {
            Some(&self.update)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagedFieldSource {
    CurrentPrincipal,
    CurrentScope,
    Generated,
    StoreClock,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagedField {
    pub field: String,
    pub source: ManagedFieldSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelationSchema {
    pub name: String,
    pub target_resource: String,
    pub cardinality: Cardinality,
    pub on_delete: DeleteRule,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexIntent {
    pub fields: Vec<String>,
    pub unique: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSchema {
    pub name: String,
    pub tenant_isolation: TenantIsolation,
    pub fields: Vec<FieldSchema>,
    pub field_policies: Vec<FieldPolicy>,
    pub managed_fields: Vec<ManagedField>,
    pub relations: Vec<RelationSchema>,
    pub indexes: Vec<IndexIntent>,
    pub policies: PolicySet,
}

impl ResourceSchema {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            tenant_isolation: TenantIsolation::CurrentScope,
            fields: Vec::new(),
            field_policies: Vec::new(),
            managed_fields: Vec::new(),
            relations: Vec::new(),
            indexes: Vec::new(),
            policies: PolicySet::default(),
        }
    }

    pub fn action_is_explicitly_authorized(&self, action: &Action) -> bool {
        self.policies.rule_for(action).is_some()
    }

    pub fn field(&self, name: &str) -> Option<&FieldSchema> {
        self.fields.iter().find(|field| field.name == name)
    }

    pub fn field_policy(&self, name: &str) -> Option<&FieldPolicy> {
        self.field_policies
            .iter()
            .find(|policy| policy.field == name)
    }

    pub fn managed_field(&self, name: &str) -> Option<&ManagedField> {
        self.managed_fields
            .iter()
            .find(|binding| binding.field == name)
    }

    fn is_global(&self) -> bool {
        self.tenant_isolation == TenantIsolation::Global
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageSchemaIssue {
    DuplicateResource(String),
    DuplicateField(String),
    DuplicateFieldPolicy(String),
    UnknownFieldPolicyTarget(String),
    DuplicateManagedField(String),
    UnknownManagedFieldTarget(String),
    ManagedFieldMustBeImmutable(String),
    ManagedFieldTypeMismatch(String),
    DuplicateRelation(String),
    DuplicateMemberName(String),
    DuplicatePolicyAction(String),
    UnknownIndexField(String),
    ProtectedFieldRequiresProtectedIndex(String),
    UnknownRelationTarget { relation: String, target: String },
    CredentialFieldMustUseCredentialType(String),
    SecretFieldMustUseSecretType(String),
}

pub fn validate_resource_schema(resource: &ResourceSchema) -> Result<(), Vec<StorageSchemaIssue>> {
    let mut issues = Vec::new();
    let mut field_names = BTreeSet::new();
    let mut relation_names = BTreeSet::new();
    let mut policy_actions = BTreeSet::new();

    for field in &resource.fields {
        if !field_names.insert(field.name.as_str()) {
            issues.push(StorageSchemaIssue::DuplicateField(field.name.clone()));
        }

        match field.protection {
            FieldProtection::Credential if !matches!(&field.ty, SemanticType::Credential(_)) => {
                issues.push(StorageSchemaIssue::CredentialFieldMustUseCredentialType(
                    field.name.clone(),
                ));
            }
            FieldProtection::Secret if !matches!(&field.ty, SemanticType::Secret(_)) => {
                issues.push(StorageSchemaIssue::SecretFieldMustUseSecretType(
                    field.name.clone(),
                ));
            }
            _ => {}
        }
    }

    let mut field_policy_names = BTreeSet::new();
    for policy in &resource.field_policies {
        if !field_policy_names.insert(policy.field.as_str()) {
            issues.push(StorageSchemaIssue::DuplicateFieldPolicy(
                policy.field.clone(),
            ));
        }
        if !field_names.contains(policy.field.as_str()) {
            issues.push(StorageSchemaIssue::UnknownFieldPolicyTarget(
                policy.field.clone(),
            ));
        }
    }

    let mut managed_field_names = BTreeSet::new();
    for binding in &resource.managed_fields {
        if !managed_field_names.insert(binding.field.as_str()) {
            issues.push(StorageSchemaIssue::DuplicateManagedField(
                binding.field.clone(),
            ));
        }

        let Some(field) = resource.field(&binding.field) else {
            issues.push(StorageSchemaIssue::UnknownManagedFieldTarget(
                binding.field.clone(),
            ));
            continue;
        };

        if field.mutable {
            issues.push(StorageSchemaIssue::ManagedFieldMustBeImmutable(
                binding.field.clone(),
            ));
        }

        let type_ok = match binding.source {
            ManagedFieldSource::CurrentPrincipal
            | ManagedFieldSource::CurrentScope => {
                field.ty == SemanticType::Text
            }
            ManagedFieldSource::StoreClock => {
                matches!(field.ty, SemanticType::Integer(_))
            }
            ManagedFieldSource::Generated => true,
        };

        if !type_ok {
            issues.push(StorageSchemaIssue::ManagedFieldTypeMismatch(
                binding.field.clone(),
            ));
        }
    }

    for relation in &resource.relations {
        if !relation_names.insert(relation.name.as_str()) {
            issues.push(StorageSchemaIssue::DuplicateRelation(relation.name.clone()));
        }
        if field_names.contains(relation.name.as_str()) {
            issues.push(StorageSchemaIssue::DuplicateMemberName(
                relation.name.clone(),
            ));
        }
    }

    for rule in &resource.policies.rules {
        if !policy_actions.insert(rule.action.0.as_str()) {
            issues.push(StorageSchemaIssue::DuplicatePolicyAction(
                rule.action.0.clone(),
            ));
        }
    }

    for index in &resource.indexes {
        for field in &index.fields {
            if !field_names.contains(field.as_str()) {
                issues.push(StorageSchemaIssue::UnknownIndexField(field.clone()));
                continue;
            }
            if let Some(schema) = resource.field(field)
                && schema.protection != FieldProtection::Public
            {
                issues.push(
                    StorageSchemaIssue::ProtectedFieldRequiresProtectedIndex(
                        field.clone(),
                    ),
                );
            }
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoreSchema {
    pub resources: Vec<ResourceSchema>,
}

impl StoreSchema {
    pub fn resource(&self, name: &str) -> Option<&ResourceSchema> {
        self.resources.iter().find(|resource| resource.name == name)
    }
}

pub fn validate_store_schema(store: &StoreSchema) -> Result<(), Vec<StorageSchemaIssue>> {
    let mut issues = Vec::new();
    let mut resource_names = BTreeSet::new();

    for resource in &store.resources {
        if !resource_names.insert(resource.name.as_str()) {
            issues.push(StorageSchemaIssue::DuplicateResource(resource.name.clone()));
        }
        if let Err(resource_issues) = validate_resource_schema(resource) {
            issues.extend(resource_issues);
        }
    }

    for resource in &store.resources {
        for relation in &resource.relations {
            if !resource_names.contains(relation.target_resource.as_str()) {
                issues.push(StorageSchemaIssue::UnknownRelationTarget {
                    relation: format!("{}.{}", resource.name, relation.name),
                    target: relation.target_resource.clone(),
                });
            }
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreOperation {
    Read { fields: Vec<String> },
    Create { fields: Vec<String> },
    Update { fields: Vec<String> },
    Delete,
    Enumerate,
}

impl StoreOperation {
    fn action(&self) -> Action {
        match self {
            Self::Read { .. } => Action::read(),
            Self::Create { .. } => Action::create(),
            Self::Update { .. } => Action::update(),
            Self::Delete => Action::delete(),
            Self::Enumerate => Action::enumerate(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreAccessIssue {
    AuthorizationDenied(DenyReason),
    UnknownField(String),
    ImmutableField(String),
    CredentialReadForbidden(String),
    CredentialMutationRequiresVerifier(String),
    ManagedFieldCannotBeSupplied(String),
    FieldPolicyDenied {
        field: String,
        reason: DenyReason,
    },
    MutationProofNotApplicable,
    Freshness(FreshnessIssue),
}

fn authorize_field_action(
    schema: &ResourceSchema,
    principal: &Principal,
    resource: &ResourceContext,
    field: &str,
    action: &Action,
) -> Option<StoreAccessIssue> {
    let policy = schema.field_policy(field)?;
    let rule = policy.rule_for(action)?;

    match rule {
        FieldAccessRule::Inherit => None,
        FieldAccessRule::Deny => Some(StoreAccessIssue::FieldPolicyDenied {
            field: field.to_owned(),
            reason: DenyReason::PolicyUnsatisfied,
        }),
        FieldAccessRule::Require(expr) => {
            let policies = PolicySet {
                rules: vec![PolicyRule {
                    action: action.clone(),
                    allow_if: expr.clone(),
                }],
            };

            match authorize(
                principal,
                resource,
                &policies,
                action,
                schema.is_global(),
            ) {
                AuthorizationDecision::Allow => None,
                AuthorizationDecision::Deny(reason) => {
                    Some(StoreAccessIssue::FieldPolicyDenied {
                        field: field.to_owned(),
                        reason,
                    })
                }
            }
        }
    }
}

pub fn authorize_store_operation(
    schema: &ResourceSchema,
    principal: &Principal,
    resource: &ResourceContext,
    operation: &StoreOperation,
) -> Result<(), Vec<StoreAccessIssue>> {
    let mut issues = Vec::new();

    match authorize(
        principal,
        resource,
        &schema.policies,
        &operation.action(),
        schema.is_global(),
    ) {
        AuthorizationDecision::Allow => {}
        AuthorizationDecision::Deny(reason) => {
            issues.push(StoreAccessIssue::AuthorizationDenied(reason));
        }
    }

    match operation {
        StoreOperation::Read { fields } => {
            for name in fields {
                match schema.field(name) {
                    None => issues.push(StoreAccessIssue::UnknownField(name.clone())),
                    Some(field) if field.protection == FieldProtection::Credential => {
                        issues.push(StoreAccessIssue::CredentialReadForbidden(name.clone()));
                    }
                    Some(_) => {
                        if let Some(issue) = authorize_field_action(
                            schema,
                            principal,
                            resource,
                            name,
                            &Action::read(),
                        ) {
                            issues.push(issue);
                        }
                    }
                }
            }
        }
        StoreOperation::Create { fields } => {
            for name in fields {
                if schema.field(name).is_none() {
                    issues.push(StoreAccessIssue::UnknownField(name.clone()));
                } else if let Some(issue) = authorize_field_action(
                    schema,
                    principal,
                    resource,
                    name,
                    &Action::create(),
                ) {
                    issues.push(issue);
                }
            }
        }
        StoreOperation::Update { fields } => {
            for name in fields {
                match schema.field(name) {
                    None => issues.push(StoreAccessIssue::UnknownField(name.clone())),
                    Some(field) if field.protection == FieldProtection::Credential => {
                        issues.push(StoreAccessIssue::CredentialMutationRequiresVerifier(
                            name.clone(),
                        ));
                    }
                    Some(field) if !field.mutable => {
                        issues.push(StoreAccessIssue::ImmutableField(name.clone()));
                    }
                    Some(_) => {
                        if let Some(issue) = authorize_field_action(
                            schema,
                            principal,
                            resource,
                            name,
                            &Action::update(),
                        ) {
                            issues.push(issue);
                        }
                    }
                }
            }
        }
        StoreOperation::Delete | StoreOperation::Enumerate => {}
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn authorize_store_mutation(
    schema: &ResourceSchema,
    principal: &Principal,
    resource: &ResourceContext,
    operation: &StoreOperation,
    epoch: SecurityEpoch,
) -> Result<MutationAuthorizationProof, Vec<StoreAccessIssue>> {
    if !matches!(
        operation,
        StoreOperation::Update { .. } | StoreOperation::Delete
    ) {
        return Err(vec![StoreAccessIssue::MutationProofNotApplicable]);
    }

    authorize_store_operation(schema, principal, resource, operation)?;

    issue_mutation_proof(
        principal,
        resource,
        &schema.policies,
        &operation.action(),
        schema.is_global(),
        epoch,
    )
    .map_err(|issue| vec![StoreAccessIssue::Freshness(issue)])
}

pub fn commit_store_mutation(
    proof: MutationAuthorizationProof,
    principal: &Principal,
    resource: &ResourceContext,
    operation: &StoreOperation,
    current_epoch: SecurityEpoch,
) -> Result<(), StoreAccessIssue> {
    if !matches!(
        operation,
        StoreOperation::Update { .. } | StoreOperation::Delete
    ) {
        return Err(StoreAccessIssue::MutationProofNotApplicable);
    }

    consume_mutation_proof(
        proof,
        principal,
        resource,
        &operation.action(),
        current_epoch,
    )
    .map_err(StoreAccessIssue::Freshness)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::authority::{PolicyExpr, PolicyRule, PrincipalId, ResourceContext, ScopeExpr};
    use crate::gir::{CapabilityClass, SemanticType};

    fn message_schema() -> ResourceSchema {
        let mut resource = ResourceSchema::new("Message");
        resource.fields = vec![
            FieldSchema {
                name: "body".into(),
                ty: SemanticType::Text,
                protection: FieldProtection::Private,
                mutable: true,
            },
            FieldSchema {
                name: "created_at".into(),
                ty: SemanticType::Integer(
                    crate::gir::IntegerType::new(0, i64::MAX as i128).unwrap(),
                ),
                protection: FieldProtection::Public,
                mutable: false,
            },
        ];
        resource
    }

    #[test]
    fn field_policy_can_restrict_row_authorized_update() {
        let mut schema = message_schema();
        schema.policies.rules.push(PolicyRule {
            action: Action::update(),
            allow_if: PolicyExpr::Public,
        });
        schema.field_policies.push(FieldPolicy {
            field: "body".into(),
            read: FieldAccessRule::Inherit,
            create: FieldAccessRule::Inherit,
            update: FieldAccessRule::Deny,
        });

        let principal = Principal::new("alice", "tenant-a");
        let resource =
            ResourceContext::new("Message", "m1", "tenant-a");

        assert_eq!(
            authorize_store_operation(
                &schema,
                &principal,
                &resource,
                &StoreOperation::Update {
                    fields: vec!["body".into()],
                },
            ),
            Err(vec![StoreAccessIssue::FieldPolicyDenied {
                field: "body".into(),
                reason: DenyReason::PolicyUnsatisfied,
            }])
        );
    }

    #[test]
    fn field_policy_can_require_owner_beyond_row_read_policy() {
        let mut schema = message_schema();
        schema.policies.rules.push(PolicyRule {
            action: Action::read(),
            allow_if: PolicyExpr::Public,
        });
        schema.field_policies.push(FieldPolicy {
            field: "body".into(),
            read: FieldAccessRule::Require(
                PolicyExpr::PrincipalOwnsResource,
            ),
            create: FieldAccessRule::Inherit,
            update: FieldAccessRule::Inherit,
        });

        let alice = Principal::new("alice", "tenant-a");
        let mallory = Principal::new("mallory", "tenant-a");
        let mut resource =
            ResourceContext::new("Message", "m1", "tenant-a");
        resource.owner = Some(alice.id.clone());

        let operation = StoreOperation::Read {
            fields: vec!["body".into()],
        };

        assert!(
            authorize_store_operation(
                &schema,
                &alice,
                &resource,
                &operation,
            )
            .is_ok()
        );
        assert!(matches!(
            authorize_store_operation(
                &schema,
                &mallory,
                &resource,
                &operation,
            ),
            Err(issues) if issues.iter().any(|issue| matches!(
                issue,
                StoreAccessIssue::FieldPolicyDenied {
                    field,
                    reason: DenyReason::PolicyUnsatisfied,
                } if field == "body"
            ))
        ));
    }

    #[test]
    fn field_policy_targets_must_exist_and_be_unique() {
        let mut schema = message_schema();
        schema.field_policies = vec![
            FieldPolicy::inherit("missing"),
            FieldPolicy::inherit("missing"),
        ];

        let issues = validate_resource_schema(&schema).unwrap_err();
        assert!(issues.contains(
            &StorageSchemaIssue::UnknownFieldPolicyTarget(
                "missing".into(),
            )
        ));
        assert!(issues.contains(
            &StorageSchemaIssue::DuplicateFieldPolicy(
                "missing".into(),
            )
        ));
    }

    #[test]
    fn resources_default_to_scoped_isolation_and_deny() {
        let resource = ResourceSchema::new("Message");
        assert_eq!(resource.tenant_isolation, TenantIsolation::CurrentScope);
        assert!(!resource.action_is_explicitly_authorized(&Action::read()));
    }

    #[test]
    fn credential_storage_cannot_be_plain_text() {
        let mut resource = ResourceSchema::new("User");
        resource.fields.push(FieldSchema {
            name: "password".into(),
            ty: SemanticType::Text,
            protection: FieldProtection::Credential,
            mutable: true,
        });

        let issues = validate_resource_schema(&resource).unwrap_err();
        assert!(issues.iter().any(|issue| {
            matches!(
                issue,
                StorageSchemaIssue::CredentialFieldMustUseCredentialType(name)
                if name == "password"
            )
        }));
    }

    #[test]
    fn secret_storage_requires_structural_secret_type() {
        let mut resource = ResourceSchema::new("ApiToken");
        resource.fields.push(FieldSchema {
            name: "token".into(),
            ty: SemanticType::Secret(Box::new(SemanticType::Bytes)),
            protection: FieldProtection::Secret,
            mutable: true,
        });

        assert!(validate_resource_schema(&resource).is_ok());
    }

    #[test]
    fn relation_targets_are_verified_by_the_store_schema() {
        let mut message = ResourceSchema::new("Message");
        message.relations.push(RelationSchema {
            name: "author".into(),
            target_resource: "User".into(),
            cardinality: Cardinality::One,
            on_delete: DeleteRule::Restrict,
        });

        let store = StoreSchema {
            resources: vec![message],
        };

        let issues = validate_store_schema(&store).unwrap_err();
        assert!(issues.iter().any(|issue| {
            matches!(
                issue,
                StorageSchemaIssue::UnknownRelationTarget { relation, target }
                if relation == "Message.author" && target == "User"
            )
        }));
    }

    #[test]
    fn relation_targets_validate_when_resource_exists() {
        let user = ResourceSchema::new("User");
        let mut message = ResourceSchema::new("Message");
        message.relations.push(RelationSchema {
            name: "author".into(),
            target_resource: "User".into(),
            cardinality: Cardinality::One,
            on_delete: DeleteRule::Restrict,
        });

        let store = StoreSchema {
            resources: vec![user, message],
        };

        assert!(validate_store_schema(&store).is_ok());
    }

    #[test]
    fn private_fields_request_native_field_protection() {
        assert_eq!(
            FieldProtection::Private.encryption_requirement(),
            EncryptionRequirement::FieldProtected
        );
        assert_eq!(
            FieldProtection::Credential.encryption_requirement(),
            EncryptionRequirement::VerifierOnly
        );
    }

    #[test]
    fn valid_identity_does_not_bypass_missing_resource_policy() {
        let schema = message_schema();
        let principal = Principal::new("alice", "tenant-a");
        let mut message = ResourceContext::new("Message", "m1", "tenant-a");
        message.owner = Some(principal.id.clone());

        let issues = authorize_store_operation(
            &schema,
            &principal,
            &message,
            &StoreOperation::Read {
                fields: vec!["body".into()],
            },
        )
        .unwrap_err();

        assert!(issues.iter().any(|issue| {
            matches!(
                issue,
                StoreAccessIssue::AuthorizationDenied(DenyReason::NoPolicy)
            )
        }));
    }

    #[test]
    fn owner_policy_does_not_cross_scope() {
        let mut schema = message_schema();
        schema.policies.rules.push(PolicyRule {
            action: Action::read(),
            allow_if: PolicyExpr::PrincipalOwnsResource,
        });

        let principal = Principal::new("alice", "tenant-a");
        let mut message = ResourceContext::new("Message", "m1", "tenant-b");
        message.owner = Some(principal.id.clone());

        let issues = authorize_store_operation(
            &schema,
            &principal,
            &message,
            &StoreOperation::Read {
                fields: vec!["body".into()],
            },
        )
        .unwrap_err();

        assert!(issues.iter().any(|issue| {
            matches!(
                issue,
                StoreAccessIssue::AuthorizationDenied(DenyReason::ScopeMismatch)
            )
        }));
    }

    #[test]
    fn credential_field_can_never_be_read_back() {
        let mut schema = ResourceSchema::new("User");
        schema.fields.push(FieldSchema {
            name: "password".into(),
            ty: SemanticType::Credential(Box::new(SemanticType::Text)),
            protection: FieldProtection::Credential,
            mutable: true,
        });
        schema.policies.rules.push(PolicyRule {
            action: Action::read(),
            allow_if: PolicyExpr::PrincipalOwnsResource,
        });

        let principal = Principal::new("alice", "tenant-a");
        let mut user = ResourceContext::new("User", "alice", "tenant-a");
        user.owner = Some(PrincipalId::new("alice"));

        let issues = authorize_store_operation(
            &schema,
            &principal,
            &user,
            &StoreOperation::Read {
                fields: vec!["password".into()],
            },
        )
        .unwrap_err();

        assert!(issues.iter().any(|issue| {
            matches!(
                issue,
                StoreAccessIssue::CredentialReadForbidden(field)
                if field == "password"
            )
        }));
    }

    #[test]
    fn enumerate_requires_its_own_policy_not_read_policy() {
        let mut schema = message_schema();
        schema.policies.rules.push(PolicyRule {
            action: Action::read(),
            allow_if: PolicyExpr::PrincipalOwnsResource,
        });

        let principal = Principal::new("alice", "tenant-a");
        let mut message = ResourceContext::new("Message", "m1", "tenant-a");
        message.owner = Some(principal.id.clone());

        let issues =
            authorize_store_operation(&schema, &principal, &message, &StoreOperation::Enumerate)
                .unwrap_err();

        assert!(issues.iter().any(|issue| {
            matches!(
                issue,
                StoreAccessIssue::AuthorizationDenied(DenyReason::NoPolicy)
            )
        }));
    }

    #[test]
    fn store_mutation_rejects_stale_authorization_proof() {
        let mut schema = message_schema();
        schema.policies.rules.push(PolicyRule {
            action: Action::update(),
            allow_if: PolicyExpr::PrincipalOwnsResource,
        });

        let principal = Principal::new("alice", "tenant-a");
        let mut message = ResourceContext::new("Message", "m1", "tenant-a");
        message.owner = Some(principal.id.clone());
        let operation = StoreOperation::Update {
            fields: vec!["body".into()],
        };
        let authorized_at = SecurityEpoch::new(4, 2, 9);

        let proof =
            authorize_store_mutation(&schema, &principal, &message, &operation, authorized_at)
                .unwrap();

        let current = SecurityEpoch::new(4, 3, 9);
        assert_eq!(
            commit_store_mutation(proof, &principal, &message, &operation, current,),
            Err(StoreAccessIssue::Freshness(
                FreshnessIssue::SecurityStateChanged {
                    authorized_at,
                    current,
                }
            ))
        );
    }

    #[test]
    fn store_mutation_accepts_fresh_consumed_proof() {
        let mut schema = message_schema();
        schema.policies.rules.push(PolicyRule {
            action: Action::update(),
            allow_if: PolicyExpr::PrincipalOwnsResource,
        });

        let principal = Principal::new("alice", "tenant-a");
        let mut message = ResourceContext::new("Message", "m1", "tenant-a");
        message.owner = Some(principal.id.clone());
        let operation = StoreOperation::Update {
            fields: vec!["body".into()],
        };
        let epoch = SecurityEpoch::new(4, 2, 9);

        let proof =
            authorize_store_mutation(&schema, &principal, &message, &operation, epoch).unwrap();

        assert!(commit_store_mutation(proof, &principal, &message, &operation, epoch,).is_ok());
    }

    #[test]
    fn matching_resource_capability_can_authorize_store_read() {
        let required = crate::authority::CapabilityGrant {
            class: CapabilityClass::Resource,
            action: Action::read(),
            resource: crate::authority::ResourceKind::new("Message"),
            scope: ScopeExpr::CurrentScope,
        };

        let mut schema = message_schema();
        schema.policies.rules.push(PolicyRule {
            action: Action::read(),
            allow_if: PolicyExpr::Requires(required.clone()),
        });

        let mut principal = Principal::new("alice", "tenant-a");
        principal.grants.insert(required);

        let message = ResourceContext::new("Message", "m1", "tenant-a");
        assert!(
            authorize_store_operation(
                &schema,
                &principal,
                &message,
                &StoreOperation::Read {
                    fields: vec!["body".into()],
                },
            )
            .is_ok()
        );
    }
}
