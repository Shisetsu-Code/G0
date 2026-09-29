use crate::gir::Effect;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplaySafety {
    /// Repeating the operation has the same externally visible effect.
    Idempotent,
    /// Repetition is safe only when the same idempotency identity is reused.
    Deduplicated { key_domain: String },
    /// Repetition is governed by an atomic transaction/commit protocol.
    Transactional,
    /// Repetition is forbidden by the semantic contract.
    NeverReplay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectContract {
    pub name: String,
    pub effect: Effect,
    pub replay: ReplaySafety,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts including the first execution.
    pub max_attempts: u32,
}

impl RetryPolicy {
    pub fn once() -> Self {
        Self { max_attempts: 1 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetryIssue {
    ZeroAttempts,
    UnsafeReplay {
        effect: String,
        max_attempts: u32,
    },
    MissingIdempotencyIdentity {
        effect: String,
    },
}

pub fn validate_retry(
    contract: &EffectContract,
    policy: RetryPolicy,
    has_idempotency_identity: bool,
) -> Result<(), Vec<RetryIssue>> {
    let mut issues = Vec::new();

    if policy.max_attempts == 0 {
        issues.push(RetryIssue::ZeroAttempts);
    }

    if policy.max_attempts > 1 {
        match &contract.replay {
            ReplaySafety::Idempotent | ReplaySafety::Transactional => {}
            ReplaySafety::Deduplicated { .. } if has_idempotency_identity => {}
            ReplaySafety::Deduplicated { .. } => {
                issues.push(RetryIssue::MissingIdempotencyIdentity {
                    effect: contract.name.clone(),
                });
            }
            ReplaySafety::NeverReplay => {
                issues.push(RetryIssue::UnsafeReplay {
                    effect: contract.name.clone(),
                    max_attempts: policy.max_attempts,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_replayable_payment_cannot_retry() {
        let payment = EffectContract {
            name: "Payment.create".into(),
            effect: Effect::Network,
            replay: ReplaySafety::NeverReplay,
        };

        let issues = validate_retry(
            &payment,
            RetryPolicy { max_attempts: 3 },
            false,
        )
        .unwrap_err();

        assert!(issues.iter().any(|issue| {
            matches!(
                issue,
                RetryIssue::UnsafeReplay {
                    effect,
                    max_attempts: 3
                } if effect == "Payment.create"
            )
        }));
    }

    #[test]
    fn deduplicated_effect_requires_identity_for_retry() {
        let message = EffectContract {
            name: "Message.send".into(),
            effect: Effect::Network,
            replay: ReplaySafety::Deduplicated {
                key_domain: "MessageSend".into(),
            },
        };

        assert!(
            validate_retry(
                &message,
                RetryPolicy { max_attempts: 4 },
                false,
            )
            .is_err()
        );
        assert!(
            validate_retry(
                &message,
                RetryPolicy { max_attempts: 4 },
                true,
            )
            .is_ok()
        );
    }

    #[test]
    fn idempotent_read_can_retry() {
        let read = EffectContract {
            name: "Profile.read".into(),
            effect: Effect::Storage,
            replay: ReplaySafety::Idempotent,
        };

        assert!(
            validate_retry(
                &read,
                RetryPolicy { max_attempts: 5 },
                false,
            )
            .is_ok()
        );
    }
}
