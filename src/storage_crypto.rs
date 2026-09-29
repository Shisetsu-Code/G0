use std::collections::BTreeSet;

use crate::storage::{FieldProtection, StoreSchema};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyScope {
    Store,
    Tenant,
    Resource,
    Field,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageCryptoProfile {
    pub id: String,
    pub minimum_security_bits: u16,
    pub current_key_version: u32,
    pub bind_tenant_as_aad: bool,
    pub bind_resource_as_aad: bool,
    pub bind_field_as_aad: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtectedFieldBinding {
    pub resource: String,
    pub field: String,
    pub profile_id: String,
    pub key_scope: KeyScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageCryptoIssue {
    UnknownResource(String),
    UnknownField(String),
    PublicFieldBindingUnnecessary(String),
    CredentialMustRemainVerifierOnly(String),
    ProfileMismatch(String),
    ZeroKeyVersion,
    SecurityFloorTooLow {
        configured: u16,
        required: u16,
    },
    MissingTenantBinding,
    MissingResourceBinding,
    MissingFieldBinding,
    MissingProtectedFieldBinding {
        resource: String,
        field: String,
    },
    DuplicateProtectedFieldBinding {
        resource: String,
        field: String,
    },
}

pub fn validate_storage_crypto_profile(
    profile: &StorageCryptoProfile,
    required_security_bits: u16,
) -> Result<(), Vec<StorageCryptoIssue>> {
    let mut issues = Vec::new();

    if profile.current_key_version == 0 {
        issues.push(StorageCryptoIssue::ZeroKeyVersion);
    }
    if profile.minimum_security_bits < required_security_bits {
        issues.push(StorageCryptoIssue::SecurityFloorTooLow {
            configured: profile.minimum_security_bits,
            required: required_security_bits,
        });
    }
    if !profile.bind_tenant_as_aad {
        issues.push(StorageCryptoIssue::MissingTenantBinding);
    }
    if !profile.bind_resource_as_aad {
        issues.push(StorageCryptoIssue::MissingResourceBinding);
    }
    if !profile.bind_field_as_aad {
        issues.push(StorageCryptoIssue::MissingFieldBinding);
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn validate_field_binding(
    store: &StoreSchema,
    profile: &StorageCryptoProfile,
    binding: &ProtectedFieldBinding,
) -> Result<(), Vec<StorageCryptoIssue>> {
    let Some(resource) = store.resource(&binding.resource) else {
        return Err(vec![StorageCryptoIssue::UnknownResource(
            binding.resource.clone(),
        )]);
    };
    let Some(field) = resource.field(&binding.field) else {
        return Err(vec![StorageCryptoIssue::UnknownField(
            binding.field.clone(),
        )]);
    };

    let mut issues = Vec::new();
    if binding.profile_id != profile.id {
        issues.push(StorageCryptoIssue::ProfileMismatch(
            binding.field.clone(),
        ));
    }

    match field.protection {
        FieldProtection::Public => {
            issues.push(StorageCryptoIssue::PublicFieldBindingUnnecessary(
                binding.field.clone(),
            ));
        }
        FieldProtection::Credential => {
            issues.push(StorageCryptoIssue::CredentialMustRemainVerifierOnly(
                binding.field.clone(),
            ));
        }
        FieldProtection::Private | FieldProtection::Secret => {}
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn validate_binding_coverage(
    store: &StoreSchema,
    bindings: &[ProtectedFieldBinding],
) -> Result<(), Vec<StorageCryptoIssue>> {
    let required = required_bindings(store);
    let mut seen = BTreeSet::new();
    let mut issues = Vec::new();

    for binding in bindings {
        let key = (binding.resource.clone(), binding.field.clone());
        if !seen.insert(key.clone()) {
            issues.push(StorageCryptoIssue::DuplicateProtectedFieldBinding {
                resource: key.0,
                field: key.1,
            });
        }
    }

    for (resource, field) in required {
        if !seen.contains(&(resource.clone(), field.clone())) {
            issues.push(StorageCryptoIssue::MissingProtectedFieldBinding {
                resource,
                field,
            });
        }
    }

    if issues.is_empty() { Ok(()) } else { Err(issues) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationMode {
    RewriteInBackground,
    ReencryptOnRead,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRotationPlan {
    pub profile_id: String,
    pub from_version: u32,
    pub to_version: u32,
    pub mode: RotationMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RotationIssue {
    WrongProfile,
    NonIncreasingVersion,
    TargetNotCurrentProfileVersion,
}

pub fn validate_rotation(
    profile: &StorageCryptoProfile,
    rotation: &KeyRotationPlan,
) -> Result<(), Vec<RotationIssue>> {
    let mut issues = Vec::new();

    if rotation.profile_id != profile.id {
        issues.push(RotationIssue::WrongProfile);
    }
    if rotation.to_version <= rotation.from_version {
        issues.push(RotationIssue::NonIncreasingVersion);
    }
    if rotation.to_version != profile.current_key_version {
        issues.push(RotationIssue::TargetNotCurrentProfileVersion);
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn required_bindings(store: &StoreSchema) -> BTreeSet<(String, String)> {
    let mut result = BTreeSet::new();
    for resource in &store.resources {
        for field in &resource.fields {
            if matches!(
                field.protection,
                FieldProtection::Private | FieldProtection::Secret
            ) {
                result.insert((resource.name.clone(), field.name.clone()));
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gir::SemanticType;
    use crate::storage::{FieldSchema, ResourceSchema};

    fn store() -> StoreSchema {
        let mut user = ResourceSchema::new("User");
        user.fields.push(FieldSchema {
            name: "email".into(),
            ty: SemanticType::Text,
            protection: FieldProtection::Private,
            mutable: true,
        });
        StoreSchema {
            resources: vec![user],
        }
    }

    fn profile() -> StorageCryptoProfile {
        StorageCryptoProfile {
            id: "store.secure".into(),
            minimum_security_bits: 192,
            current_key_version: 3,
            bind_tenant_as_aad: true,
            bind_resource_as_aad: true,
            bind_field_as_aad: true,
        }
    }

    #[test]
    fn private_field_crypto_is_bound_to_storage_not_app_code() {
        let binding = ProtectedFieldBinding {
            resource: "User".into(),
            field: "email".into(),
            profile_id: "store.secure".into(),
            key_scope: KeyScope::Tenant,
        };

        assert!(
            validate_storage_crypto_profile(&profile(), 128).is_ok()
        );
        assert!(
            validate_field_binding(&store(), &profile(), &binding).is_ok()
        );
    }

    #[test]
    fn every_private_field_requires_crypto_binding() {
        assert!(matches!(
            validate_binding_coverage(&store(), &[]),
            Err(issues) if issues.contains(
                &StorageCryptoIssue::MissingProtectedFieldBinding {
                    resource: "User".into(),
                    field: "email".into(),
                }
            )
        ));
    }

    #[test]
    fn storage_ciphertext_is_context_bound() {
        let mut weak = profile();
        weak.bind_tenant_as_aad = false;

        assert!(validate_storage_crypto_profile(&weak, 128)
            .unwrap_err()
            .contains(&StorageCryptoIssue::MissingTenantBinding));
    }

    #[test]
    fn rotation_is_versioned_without_application_rewrite() {
        let rotation = KeyRotationPlan {
            profile_id: "store.secure".into(),
            from_version: 2,
            to_version: 3,
            mode: RotationMode::RewriteInBackground,
        };

        assert!(validate_rotation(&profile(), &rotation).is_ok());
    }
}
