use std::collections::BTreeSet;

use crate::gir::CapabilityClass;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ArtifactId(pub String);

impl ArtifactId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContentHash(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct SignerIdentity(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct InterfaceId(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactMode {
    /// Definitions/types only. This artifact may be imported but never executed.
    DefinitionOnly,
    /// Executable artifact; still requires an explicit execution grant.
    Executable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactManifest {
    pub id: ArtifactId,
    pub content_hash: ContentHash,
    pub signer: SignerIdentity,
    pub interface: InterfaceId,
    pub mode: ArtifactMode,
    pub requested_capabilities: BTreeSet<CapabilityClass>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionGrant {
    pub target: String,
    pub artifact: ArtifactId,
    pub content_hash: ContentHash,
    pub interface: InterfaceId,
    pub granted_capabilities: BTreeSet<CapabilityClass>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArtifactIssue {
    DefinitionOnlyCannotExecute,
    ArtifactIdentityMismatch,
    ContentHashMismatch,
    InterfaceMismatch,
    CapabilityNotGranted(CapabilityClass),
}

pub fn validate_execution_grant(
    manifest: &ArtifactManifest,
    grant: &ExecutionGrant,
) -> Result<(), Vec<ArtifactIssue>> {
    let mut issues = Vec::new();

    if manifest.mode != ArtifactMode::Executable {
        issues.push(ArtifactIssue::DefinitionOnlyCannotExecute);
    }
    if manifest.id != grant.artifact {
        issues.push(ArtifactIssue::ArtifactIdentityMismatch);
    }
    if manifest.content_hash != grant.content_hash {
        issues.push(ArtifactIssue::ContentHashMismatch);
    }
    if manifest.interface != grant.interface {
        issues.push(ArtifactIssue::InterfaceMismatch);
    }

    for capability in &manifest.requested_capabilities {
        if !grant.granted_capabilities.contains(capability) {
            issues.push(ArtifactIssue::CapabilityNotGranted(*capability));
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportHandle {
    pub artifact: ArtifactId,
    pub interface: InterfaceId,
}

pub fn import_definitions(manifest: &ArtifactManifest) -> ImportHandle {
    ImportHandle {
        artifact: manifest.id.clone(),
        interface: manifest.interface.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(mode: ArtifactMode) -> ArtifactManifest {
        ArtifactManifest {
            id: ArtifactId::new("image-filter"),
            content_hash: ContentHash("abc123".into()),
            signer: SignerIdentity("publisher-key-1".into()),
            interface: InterfaceId("ImageFilter.v1".into()),
            mode,
            requested_capabilities: [CapabilityClass::Accelerator]
                .into_iter()
                .collect(),
        }
    }

    #[test]
    fn importing_definitions_never_executes_or_grants_authority() {
        let handle = import_definitions(&manifest(ArtifactMode::DefinitionOnly));
        assert_eq!(handle.artifact, ArtifactId::new("image-filter"));
    }

    #[test]
    fn definition_only_artifact_cannot_be_executed() {
        let artifact = manifest(ArtifactMode::DefinitionOnly);
        let grant = ExecutionGrant {
            target: "worker-a".into(),
            artifact: artifact.id.clone(),
            content_hash: artifact.content_hash.clone(),
            interface: artifact.interface.clone(),
            granted_capabilities: [CapabilityClass::Accelerator]
                .into_iter()
                .collect(),
        };

        assert!(matches!(
            validate_execution_grant(&artifact, &grant)
                .unwrap_err()
                .first(),
            Some(ArtifactIssue::DefinitionOnlyCannotExecute)
        ));
    }

    #[test]
    fn same_filename_or_name_is_irrelevant_when_hash_differs() {
        let artifact = manifest(ArtifactMode::Executable);
        let grant = ExecutionGrant {
            target: "worker-a".into(),
            artifact: artifact.id.clone(),
            content_hash: ContentHash("changed-content".into()),
            interface: artifact.interface.clone(),
            granted_capabilities: [CapabilityClass::Accelerator]
                .into_iter()
                .collect(),
        };

        assert!(validate_execution_grant(&artifact, &grant)
            .unwrap_err()
            .contains(&ArtifactIssue::ContentHashMismatch));
    }

    #[test]
    fn artifact_cannot_gain_undeclared_or_ungranted_authority() {
        let artifact = manifest(ArtifactMode::Executable);
        let grant = ExecutionGrant {
            target: "worker-a".into(),
            artifact: artifact.id.clone(),
            content_hash: artifact.content_hash.clone(),
            interface: artifact.interface.clone(),
            granted_capabilities: BTreeSet::new(),
        };

        assert_eq!(
            validate_execution_grant(&artifact, &grant),
            Err(vec![ArtifactIssue::CapabilityNotGranted(
                CapabilityClass::Accelerator
            )])
        );
    }
}
