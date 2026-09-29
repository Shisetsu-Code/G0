#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomnessClass {
    /// Cryptographic entropy supplied by a trusted platform source.
    SecureEntropy,
    /// Reproducible pseudorandom stream for tests, simulation and optimization.
    Deterministic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RandomUse {
    Credential,
    SessionKey,
    Nonce,
    Identifier,
    Simulation,
    Test,
    Optimizer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntropyIssue {
    DeterministicRandomnessUsedForSecurity(RandomUse),
    SecureEntropyUsedWhereReproducibilityRequired(RandomUse),
}

pub fn validate_randomness_use(
    class: RandomnessClass,
    usage: RandomUse,
) -> Result<(), EntropyIssue> {
    match (class, usage) {
        (
            RandomnessClass::Deterministic,
            RandomUse::Credential
            | RandomUse::SessionKey
            | RandomUse::Nonce
            | RandomUse::Identifier,
        ) => Err(EntropyIssue::DeterministicRandomnessUsedForSecurity(
            usage,
        )),
        (
            RandomnessClass::SecureEntropy,
            RandomUse::Simulation | RandomUse::Test | RandomUse::Optimizer,
        ) => Err(EntropyIssue::SecureEntropyUsedWhereReproducibilityRequired(
            usage,
        )),
        _ => Ok(()),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeterministicSeed(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntropyRequest {
    pub bytes: u32,
    pub usage: RandomUse,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntropyRequestIssue {
    ZeroLength,
    ExcessiveSingleRequest(u32),
}

pub fn validate_entropy_request(
    request: EntropyRequest,
) -> Result<(), EntropyRequestIssue> {
    if request.bytes == 0 {
        return Err(EntropyRequestIssue::ZeroLength);
    }
    if request.bytes > 1_048_576 {
        return Err(EntropyRequestIssue::ExcessiveSingleRequest(
            request.bytes,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentials_cannot_use_deterministic_rng() {
        assert_eq!(
            validate_randomness_use(
                RandomnessClass::Deterministic,
                RandomUse::Credential,
            ),
            Err(EntropyIssue::DeterministicRandomnessUsedForSecurity(
                RandomUse::Credential
            ))
        );
    }

    #[test]
    fn optimizer_uses_reproducible_randomness() {
        assert!(
            validate_randomness_use(
                RandomnessClass::Deterministic,
                RandomUse::Optimizer,
            )
            .is_ok()
        );
    }

    #[test]
    fn huge_entropy_reads_are_not_a_normal_primitive() {
        assert_eq!(
            validate_entropy_request(EntropyRequest {
                bytes: 2_000_000,
                usage: RandomUse::Nonce,
            }),
            Err(EntropyRequestIssue::ExcessiveSingleRequest(2_000_000))
        );
    }
}
