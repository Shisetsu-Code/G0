use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageVersion(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PackageHash(pub String);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PublisherIdentity(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageManifest {
    pub id: PackageId,
    pub version: PackageVersion,
    pub content_hash: PackageHash,
    pub publisher: PublisherIdentity,
    pub dependencies: BTreeMap<PackageId, PackageRequirement>,
    pub exported_interfaces: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageRequirement {
    /// Exact version in lock resolution. No implicit floating dependency.
    pub version: PackageVersion,
    pub content_hash: PackageHash,
    pub publisher: PublisherIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PackageLock {
    pub packages: BTreeMap<PackageId, PackageRequirement>,
    pub compiler_version: String,
    pub platform_profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageIssue {
    MissingLockEntry(PackageId),
    VersionMismatch(PackageId),
    HashMismatch(PackageId),
    PublisherMismatch(PackageId),
    UndeclaredDependency(PackageId),
}

pub fn verify_package_against_lock(
    manifest: &PackageManifest,
    lock: &PackageLock,
) -> Result<(), Vec<PackageIssue>> {
    let mut issues = Vec::new();

    match lock.packages.get(&manifest.id) {
        None => issues.push(PackageIssue::MissingLockEntry(manifest.id.clone())),
        Some(expected) => {
            if expected.version != manifest.version {
                issues.push(PackageIssue::VersionMismatch(manifest.id.clone()));
            }
            if expected.content_hash != manifest.content_hash {
                issues.push(PackageIssue::HashMismatch(manifest.id.clone()));
            }
            if expected.publisher != manifest.publisher {
                issues.push(PackageIssue::PublisherMismatch(
                    manifest.id.clone(),
                ));
            }
        }
    }

    for dependency in manifest.dependencies.keys() {
        if !lock.packages.contains_key(dependency) {
            issues.push(PackageIssue::UndeclaredDependency(
                dependency.clone(),
            ));
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn dependency_requirement(
    manifest: &PackageManifest,
    dependency: &PackageId,
) -> Option<&PackageRequirement> {
    manifest.dependencies.get(dependency)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn requirement() -> PackageRequirement {
        PackageRequirement {
            version: PackageVersion("1.2.3".into()),
            content_hash: PackageHash("abc".into()),
            publisher: PublisherIdentity("publisher-a".into()),
        }
    }

    #[test]
    fn package_identity_is_version_hash_and_publisher_bound() {
        let id = PackageId("math".into());
        let manifest = PackageManifest {
            id: id.clone(),
            version: PackageVersion("1.2.3".into()),
            content_hash: PackageHash("abc".into()),
            publisher: PublisherIdentity("publisher-a".into()),
            dependencies: BTreeMap::new(),
            exported_interfaces: BTreeSet::new(),
        };
        let lock = PackageLock {
            packages: BTreeMap::from([(id, requirement())]),
            compiler_version: "g0c-0.1".into(),
            platform_profile: "server.current".into(),
        };

        assert!(verify_package_against_lock(&manifest, &lock).is_ok());
    }

    #[test]
    fn same_name_and_version_with_changed_bytes_is_rejected() {
        let id = PackageId("math".into());
        let manifest = PackageManifest {
            id: id.clone(),
            version: PackageVersion("1.2.3".into()),
            content_hash: PackageHash("tampered".into()),
            publisher: PublisherIdentity("publisher-a".into()),
            dependencies: BTreeMap::new(),
            exported_interfaces: BTreeSet::new(),
        };
        let lock = PackageLock {
            packages: BTreeMap::from([(id.clone(), requirement())]),
            compiler_version: "g0c-0.1".into(),
            platform_profile: "server.current".into(),
        };

        assert_eq!(
            verify_package_against_lock(&manifest, &lock),
            Err(vec![PackageIssue::HashMismatch(id)])
        );
    }

    #[test]
    fn dependencies_do_not_float_implicitly() {
        let dependency = PackageId("codec".into());
        let manifest = PackageManifest {
            id: PackageId("app".into()),
            version: PackageVersion("1".into()),
            content_hash: PackageHash("apphash".into()),
            publisher: PublisherIdentity("publisher-a".into()),
            dependencies: BTreeMap::from([(
                dependency.clone(),
                requirement(),
            )]),
            exported_interfaces: BTreeSet::new(),
        };
        let lock = PackageLock::default();

        let issues = verify_package_against_lock(&manifest, &lock).unwrap_err();
        assert!(issues.contains(&PackageIssue::UndeclaredDependency(
            dependency
        )));
    }
}
