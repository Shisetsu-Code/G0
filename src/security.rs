use crate::gir::ParameterPolicy;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityProfile {
    pub id: String,
    pub password_memory_floor_mib: u32,
    pub password_work_floor: u32,
    pub side_channel_resistant_verification: bool,
    pub uniform_public_auth_failure: bool,
    pub credential_storage_is_verifier_only: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordParameters {
    pub memory_mib: u32,
    pub work_factor: u32,
    pub parallelism: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordTuning {
    pub memory_mib: ParameterPolicy<u32>,
    pub work_factor: ParameterPolicy<u32>,
    pub parallelism: ParameterPolicy<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecurityContractIssue {
    ProfileAllowsSecretDependentVerification,
    ProfileAllowsDistinctPublicAuthFailures,
    ProfileAllowsRecoverableCredentialStorage,
    MemoryFloorBelowProfile {
        configured_min: u32,
        required_min: u32,
    },
    WorkFloorBelowProfile {
        configured_min: u32,
        required_min: u32,
    },
    ParameterOutsidePolicy {
        parameter: &'static str,
        value: u32,
    },
    ParameterBelowSecurityFloor {
        parameter: &'static str,
        value: u32,
        required_min: u32,
    },
}

pub fn validate_password_contract(
    profile: &SecurityProfile,
    tuning: PasswordTuning,
) -> Result<(), Vec<SecurityContractIssue>> {
    let mut issues = Vec::new();

    if !profile.side_channel_resistant_verification {
        issues.push(SecurityContractIssue::ProfileAllowsSecretDependentVerification);
    }
    if !profile.uniform_public_auth_failure {
        issues.push(SecurityContractIssue::ProfileAllowsDistinctPublicAuthFailures);
    }
    if !profile.credential_storage_is_verifier_only {
        issues.push(SecurityContractIssue::ProfileAllowsRecoverableCredentialStorage);
    }

    if policy_min(tuning.memory_mib) < profile.password_memory_floor_mib {
        issues.push(SecurityContractIssue::MemoryFloorBelowProfile {
            configured_min: policy_min(tuning.memory_mib),
            required_min: profile.password_memory_floor_mib,
        });
    }

    if policy_min(tuning.work_factor) < profile.password_work_floor {
        issues.push(SecurityContractIssue::WorkFloorBelowProfile {
            configured_min: policy_min(tuning.work_factor),
            required_min: profile.password_work_floor,
        });
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn validate_password_candidate(
    profile: &SecurityProfile,
    tuning: PasswordTuning,
    candidate: PasswordParameters,
) -> Result<(), Vec<SecurityContractIssue>> {
    let mut issues = Vec::new();

    validate_parameter(
        "memory_mib",
        tuning.memory_mib,
        candidate.memory_mib,
        &mut issues,
    );
    validate_parameter(
        "work_factor",
        tuning.work_factor,
        candidate.work_factor,
        &mut issues,
    );
    validate_parameter(
        "parallelism",
        tuning.parallelism,
        candidate.parallelism,
        &mut issues,
    );

    if candidate.memory_mib < profile.password_memory_floor_mib {
        issues.push(SecurityContractIssue::ParameterBelowSecurityFloor {
            parameter: "memory_mib",
            value: candidate.memory_mib,
            required_min: profile.password_memory_floor_mib,
        });
    }

    if candidate.work_factor < profile.password_work_floor {
        issues.push(SecurityContractIssue::ParameterBelowSecurityFloor {
            parameter: "work_factor",
            value: candidate.work_factor,
            required_min: profile.password_work_floor,
        });
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

fn policy_min(policy: ParameterPolicy<u32>) -> u32 {
    match policy {
        ParameterPolicy::Fixed(value) => value,
        ParameterPolicy::Bounded { min, .. } => min,
        ParameterPolicy::Free => 0,
    }
}

fn validate_parameter(
    name: &'static str,
    policy: ParameterPolicy<u32>,
    value: u32,
    issues: &mut Vec<SecurityContractIssue>,
) {
    let valid = match policy {
        ParameterPolicy::Fixed(required) => value == required,
        ParameterPolicy::Bounded { min, max } => value >= min && value <= max,
        ParameterPolicy::Free => true,
    };

    if !valid {
        issues.push(SecurityContractIssue::ParameterOutsidePolicy {
            parameter: name,
            value,
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicAuthenticationError {
    AuthenticationFailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalAuthenticationReason {
    UnknownIdentity,
    InvalidCredential,
    CredentialExpired,
    AccountPolicyDenied,
    RateLimited,
}

pub fn public_auth_error(
    _reason: InternalAuthenticationReason,
) -> PublicAuthenticationError {
    PublicAuthenticationError::AuthenticationFailed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn high_profile() -> SecurityProfile {
        SecurityProfile {
            id: "high".into(),
            password_memory_floor_mib: 128,
            password_work_floor: 3,
            side_channel_resistant_verification: true,
            uniform_public_auth_failure: true,
            credential_storage_is_verifier_only: true,
        }
    }

    fn tunable_password() -> PasswordTuning {
        PasswordTuning {
            memory_mib: ParameterPolicy::Bounded {
                min: 128,
                max: 1024,
            },
            work_factor: ParameterPolicy::Bounded { min: 3, max: 12 },
            parallelism: ParameterPolicy::Bounded { min: 1, max: 8 },
        }
    }

    #[test]
    fn security_floor_is_not_an_optimizer_choice() {
        let profile = high_profile();
        let weak = PasswordTuning {
            memory_mib: ParameterPolicy::Bounded { min: 32, max: 1024 },
            ..tunable_password()
        };

        assert!(validate_password_contract(&profile, weak).is_err());
    }

    #[test]
    fn optimizer_can_choose_stronger_parameters_inside_profile() {
        let profile = high_profile();
        let tuning = tunable_password();
        let candidate = PasswordParameters {
            memory_mib: 512,
            work_factor: 6,
            parallelism: 4,
        };

        assert!(validate_password_contract(&profile, tuning).is_ok());
        assert!(validate_password_candidate(&profile, tuning, candidate).is_ok());
    }

    #[test]
    fn optimizer_cannot_trade_security_floor_for_latency() {
        let profile = high_profile();
        let tuning = tunable_password();
        let candidate = PasswordParameters {
            memory_mib: 64,
            work_factor: 2,
            parallelism: 8,
        };

        let issues =
            validate_password_candidate(&profile, tuning, candidate).unwrap_err();
        assert!(issues.iter().any(|issue| {
            matches!(
                issue,
                SecurityContractIssue::ParameterBelowSecurityFloor {
                    parameter: "memory_mib",
                    ..
                }
            )
        }));
    }

    #[test]
    fn public_authentication_failure_never_enumerates_reason() {
        for reason in [
            InternalAuthenticationReason::UnknownIdentity,
            InternalAuthenticationReason::InvalidCredential,
            InternalAuthenticationReason::CredentialExpired,
            InternalAuthenticationReason::AccountPolicyDenied,
            InternalAuthenticationReason::RateLimited,
        ] {
            assert_eq!(
                public_auth_error(reason),
                PublicAuthenticationError::AuthenticationFailed
            );
        }
    }
}
