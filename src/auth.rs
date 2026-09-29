#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthenticationFactor {
    Password,
    OneTimeCode,
    DeviceKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationTiming {
    Uniform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticationProfile {
    pub id: String,
    pub required_factors: Vec<AuthenticationFactor>,
    pub timing: VerificationTiming,
    pub max_attempts: u32,
    pub session_rotation_on_success: bool,
    pub generic_public_failure: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthenticationProfileIssue {
    NoFactor,
    DuplicateFactor(AuthenticationFactor),
    ZeroAttempts,
    SessionNotRotated,
    PublicFailureLeaksReason,
}

pub fn validate_authentication_profile(
    profile: &AuthenticationProfile,
) -> Result<(), Vec<AuthenticationProfileIssue>> {
    let mut issues = Vec::new();

    if profile.required_factors.is_empty() {
        issues.push(AuthenticationProfileIssue::NoFactor);
    }
    if profile.max_attempts == 0 {
        issues.push(AuthenticationProfileIssue::ZeroAttempts);
    }
    if !profile.session_rotation_on_success {
        issues.push(AuthenticationProfileIssue::SessionNotRotated);
    }
    if !profile.generic_public_failure {
        issues.push(AuthenticationProfileIssue::PublicFailureLeaksReason);
    }

    for (index, factor) in profile.required_factors.iter().enumerate() {
        if profile.required_factors[..index].contains(factor) {
            issues.push(AuthenticationProfileIssue::DuplicateFactor(*factor));
        }
    }

    if issues.is_empty() { Ok(()) } else { Err(issues) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OneTimeCodeContract {
    pub digits: u8,
    pub lifetime_seconds: u32,
    pub max_attempts: u32,
    pub single_use: bool,
    pub atomic_consume: bool,
    pub replay_protected: bool,
    pub timing: VerificationTiming,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OneTimeCodeIssue {
    DigitsTooLow,
    ZeroLifetime,
    ZeroAttempts,
    NotSingleUse,
    NonAtomicConsume,
    ReplayNotProtected,
}

pub fn validate_one_time_code(
    contract: OneTimeCodeContract,
) -> Result<(), Vec<OneTimeCodeIssue>> {
    let mut issues = Vec::new();

    if contract.digits < 6 {
        issues.push(OneTimeCodeIssue::DigitsTooLow);
    }
    if contract.lifetime_seconds == 0 {
        issues.push(OneTimeCodeIssue::ZeroLifetime);
    }
    if contract.max_attempts == 0 {
        issues.push(OneTimeCodeIssue::ZeroAttempts);
    }
    if !contract.single_use {
        issues.push(OneTimeCodeIssue::NotSingleUse);
    }
    if !contract.atomic_consume {
        issues.push(OneTimeCodeIssue::NonAtomicConsume);
    }
    if !contract.replay_protected {
        issues.push(OneTimeCodeIssue::ReplayNotProtected);
    }

    if issues.is_empty() { Ok(()) } else { Err(issues) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalAuthResult {
    Success,
    UnknownIdentity,
    InvalidFactor,
    Expired,
    AlreadyConsumed,
    TooManyAttempts,
    PolicyDenied,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicAuthResult {
    Success,
    Failed,
}

pub fn public_result(result: InternalAuthResult) -> PublicAuthResult {
    match result {
        InternalAuthResult::Success => PublicAuthResult::Success,
        _ => PublicAuthResult::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn otp_security_is_part_of_one_contract() {
        let otp = OneTimeCodeContract {
            digits: 8,
            lifetime_seconds: 120,
            max_attempts: 5,
            single_use: true,
            atomic_consume: true,
            replay_protected: true,
            timing: VerificationTiming::Uniform,
        };
        assert!(validate_one_time_code(otp).is_ok());
    }

    #[test]
    fn public_result_does_not_enumerate_auth_failure() {
        for result in [
            InternalAuthResult::UnknownIdentity,
            InternalAuthResult::InvalidFactor,
            InternalAuthResult::Expired,
            InternalAuthResult::AlreadyConsumed,
            InternalAuthResult::TooManyAttempts,
            InternalAuthResult::PolicyDenied,
        ] {
            assert_eq!(public_result(result), PublicAuthResult::Failed);
        }
    }

    #[test]
    fn otp_must_be_atomic_single_use_and_replay_protected() {
        let otp = OneTimeCodeContract {
            digits: 6,
            lifetime_seconds: 60,
            max_attempts: 3,
            single_use: false,
            atomic_consume: false,
            replay_protected: false,
            timing: VerificationTiming::Uniform,
        };
        let issues = validate_one_time_code(otp).unwrap_err();
        assert!(issues.contains(&OneTimeCodeIssue::NotSingleUse));
        assert!(issues.contains(&OneTimeCodeIssue::NonAtomicConsume));
        assert!(issues.contains(&OneTimeCodeIssue::ReplayNotProtected));
    }
}
