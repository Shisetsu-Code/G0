use crate::storage::{FieldProtection, StoreSchema};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtectedIndexMode {
    EqualityBlindIndex,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedIndexSpec {
    pub resource: String,
    pub field: String,
    pub mode: ProtectedIndexMode,
    pub token_bits: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectedIndexIssue {
    UnknownResource(String),
    UnknownField(String),
    PublicFieldDoesNotNeedProtectedIndex(String),
    CredentialCannotBeIndexed(String),
    TokenSecurityTooLow {
        field: String,
        token_bits: u16,
        minimum_bits: u16,
    },
}

pub fn validate_protected_index(
    store: &StoreSchema,
    index: &ProtectedIndexSpec,
    minimum_token_bits: u16,
) -> Result<(), Vec<ProtectedIndexIssue>> {
    let mut issues = Vec::new();

    let Some(resource) = store.resource(&index.resource) else {
        return Err(vec![ProtectedIndexIssue::UnknownResource(
            index.resource.clone(),
        )]);
    };

    let Some(field) = resource.field(&index.field) else {
        return Err(vec![ProtectedIndexIssue::UnknownField(
            index.field.clone(),
        )]);
    };

    match field.protection {
        FieldProtection::Public => {
            issues.push(ProtectedIndexIssue::PublicFieldDoesNotNeedProtectedIndex(
                index.field.clone(),
            ));
        }
        FieldProtection::Credential => {
            issues.push(ProtectedIndexIssue::CredentialCannotBeIndexed(
                index.field.clone(),
            ));
        }
        FieldProtection::Private | FieldProtection::Secret => {}
    }

    if index.token_bits < minimum_token_bits {
        issues.push(ProtectedIndexIssue::TokenSecurityTooLow {
            field: index.field.clone(),
            token_bits: index.token_bits,
            minimum_bits: minimum_token_bits,
        });
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtectedQuery {
    Equal {
        resource: String,
        field: String,
    },
}

pub fn protected_index_supports(
    index: &ProtectedIndexSpec,
    query: &ProtectedQuery,
) -> bool {
    match query {
        ProtectedQuery::Equal { resource, field } => {
            index.resource == *resource
                && index.field == *field
                && index.mode == ProtectedIndexMode::EqualityBlindIndex
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gir::SemanticType;
    use crate::storage::{FieldSchema, ResourceSchema, StoreSchema};

    fn store() -> StoreSchema {
        let mut user = ResourceSchema::new("User");
        user.fields.push(FieldSchema {
            name: "email".into(),
            ty: SemanticType::Text,
            protection: FieldProtection::Private,
            mutable: true,
        });
        user.fields.push(FieldSchema {
            name: "password".into(),
            ty: SemanticType::Credential(Box::new(SemanticType::Text)),
            protection: FieldProtection::Credential,
            mutable: true,
        });
        StoreSchema {
            resources: vec![user],
        }
    }

    #[test]
    fn private_field_can_use_blind_equality_index() {
        let index = ProtectedIndexSpec {
            resource: "User".into(),
            field: "email".into(),
            mode: ProtectedIndexMode::EqualityBlindIndex,
            token_bits: 192,
        };

        assert!(validate_protected_index(&store(), &index, 128).is_ok());
    }

    #[test]
    fn credential_is_never_query_indexed() {
        let index = ProtectedIndexSpec {
            resource: "User".into(),
            field: "password".into(),
            mode: ProtectedIndexMode::EqualityBlindIndex,
            token_bits: 192,
        };

        assert!(validate_protected_index(&store(), &index, 128)
            .unwrap_err()
            .contains(&ProtectedIndexIssue::CredentialCannotBeIndexed(
                "password".into()
            )));
    }

    #[test]
    fn protected_index_respects_security_floor() {
        let index = ProtectedIndexSpec {
            resource: "User".into(),
            field: "email".into(),
            mode: ProtectedIndexMode::EqualityBlindIndex,
            token_bits: 64,
        };

        assert!(validate_protected_index(&store(), &index, 128)
            .unwrap_err()
            .contains(&ProtectedIndexIssue::TokenSecurityTooLow {
                field: "email".into(),
                token_bits: 64,
                minimum_bits: 128,
            }));
    }
}
