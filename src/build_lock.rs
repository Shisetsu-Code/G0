use crate::package::PackageLock;
use crate::tuning::OptimizationLock;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildLock {
    pub program_hash: String,
    pub compiler_version: String,
    pub target_profile: String,
    pub network_profile: String,
    pub crypto_profile: String,
    pub math_profile: String,
    pub unicode_profile: String,
    pub package_lock: PackageLock,
    pub optimization_lock: Option<OptimizationLock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeFingerprint {
    pub compiler_abi: String,
    pub target_profile: String,
    pub network_profile: String,
    pub crypto_profile: String,
    pub math_profile: String,
    pub unicode_profile: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildLockIssue {
    CompilerAbiMismatch,
    TargetProfileMismatch,
    NetworkProfileMismatch,
    CryptoProfileMismatch,
    MathProfileMismatch,
    UnicodeProfileMismatch,
    OptimizationProgramHashMismatch,
}

pub fn validate_runtime(
    lock: &BuildLock,
    runtime: &RuntimeFingerprint,
) -> Result<(), Vec<BuildLockIssue>> {
    let mut issues = Vec::new();

    if lock.compiler_version != runtime.compiler_abi {
        issues.push(BuildLockIssue::CompilerAbiMismatch);
    }
    if lock.target_profile != runtime.target_profile {
        issues.push(BuildLockIssue::TargetProfileMismatch);
    }
    if lock.network_profile != runtime.network_profile {
        issues.push(BuildLockIssue::NetworkProfileMismatch);
    }
    if lock.crypto_profile != runtime.crypto_profile {
        issues.push(BuildLockIssue::CryptoProfileMismatch);
    }
    if lock.math_profile != runtime.math_profile {
        issues.push(BuildLockIssue::MathProfileMismatch);
    }
    if lock.unicode_profile != runtime.unicode_profile {
        issues.push(BuildLockIssue::UnicodeProfileMismatch);
    }

    if let Some(optimization) = &lock.optimization_lock
        && optimization.program_hash != lock.program_hash
    {
        issues.push(BuildLockIssue::OptimizationProgramHashMismatch);
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::package::PackageLock;
    use crate::tuning::OptimizationLock;

    fn lock() -> BuildLock {
        BuildLock {
            program_hash: "program-a".into(),
            compiler_version: "g0-abi-1".into(),
            target_profile: "x86_64-v3".into(),
            network_profile: "network.current".into(),
            crypto_profile: "crypto.current".into(),
            math_profile: "math.current".into(),
            unicode_profile: "unicode.current".into(),
            package_lock: PackageLock::default(),
            optimization_lock: Some(OptimizationLock {
                program_hash: "program-a".into(),
                compiler_version: "g0-abi-1".into(),
                hardware_profile_hash: "hw-a".into(),
                workload_hash: "workload-a".into(),
                selected_parameters: BTreeMap::new(),
                measured_metrics: BTreeMap::new(),
            }),
        }
    }

    #[test]
    fn runtime_profiles_are_bound_to_build() {
        let runtime = RuntimeFingerprint {
            compiler_abi: "g0-abi-1".into(),
            target_profile: "x86_64-v3".into(),
            network_profile: "network.current".into(),
            crypto_profile: "crypto.current".into(),
            math_profile: "math.current".into(),
            unicode_profile: "unicode.current".into(),
        };

        assert!(validate_runtime(&lock(), &runtime).is_ok());
    }

    #[test]
    fn silent_crypto_profile_change_is_rejected() {
        let mut runtime = RuntimeFingerprint {
            compiler_abi: "g0-abi-1".into(),
            target_profile: "x86_64-v3".into(),
            network_profile: "network.current".into(),
            crypto_profile: "crypto.current".into(),
            math_profile: "math.current".into(),
            unicode_profile: "unicode.current".into(),
        };
        runtime.crypto_profile = "crypto.legacy".into();

        assert_eq!(
            validate_runtime(&lock(), &runtime),
            Err(vec![BuildLockIssue::CryptoProfileMismatch])
        );
    }

    #[test]
    fn tuning_result_cannot_be_reused_for_different_program() {
        let mut lock = lock();
        lock.program_hash = "program-b".into();
        let runtime = RuntimeFingerprint {
            compiler_abi: "g0-abi-1".into(),
            target_profile: "x86_64-v3".into(),
            network_profile: "network.current".into(),
            crypto_profile: "crypto.current".into(),
            math_profile: "math.current".into(),
            unicode_profile: "unicode.current".into(),
        };

        assert!(validate_runtime(&lock, &runtime)
            .unwrap_err()
            .contains(&BuildLockIssue::OptimizationProgramHashMismatch));
    }
}
