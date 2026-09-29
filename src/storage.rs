use std::collections::BTreeSet;

use crate::authority::{Action, PolicySet};
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
            relations: Vec::new(),
            indexes: Vec::new(),
            policies: PolicySet::default(),
        }
    }

    pub fn action_is_explicitly_authorized(&self, action: &Action) -> bool {
        self.policies.rule_for(action).is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageSchemaIssue {
    DuplicateField(String),
    DuplicateRelation(String),
    UnknownIndexField(String),
    CredentialFieldMustUseCredentialType(String),
    SecretFieldMustUseSecretType(String),
}

pub fn validate_resource_schema(resource: &ResourceSchema) -> Result<(), Vec<StorageSchemaIssue>> {
    let mut issues = Vec::new();
    let mut field_names = BTreeSet::new();
    let mut relation_names = BTreeSet::new();

    for field in &resource.fields {
        if !field_names.insert(field.name.as_str()) {
            issues.push(StorageSchemaIssue::DuplicateField(field.name.clone()));
        }

        match field.protection {
            FieldProtection::Credential
                if !matches!(&field.ty, SemanticType::Credential(_)) =>
            {
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

    for relation in &resource.relations {
        if !relation_names.insert(relation.name.as_str()) {
            issues.push(StorageSchemaIssue::DuplicateRelation(relation.name.clone()));
        }
    }

    for index in &resource.indexes {
        for field in &index.fields {
            if !field_names.contains(field.as_str()) {
                issues.push(StorageSchemaIssue::UnknownIndexField(field.clone()));
            }
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gir::SemanticType;

    #[test]
    fn resources_default_to_scoped_isolation_and_deny() {
        let resource = ResourceSchema::new("Message");
        assert_eq!(resource.tenant_isolation, TenantIsolation::CurrentScope);
        assert!(!resource.action_is_explicitly_authorized(&Action::new("read")));
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
}
