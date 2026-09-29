#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageReplaySemantics {
    Idempotent,
    Deduplicated { identity_domain: String },
    NonReplayable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransmissionMode {
    ConfirmedSecure,
    ReplayableEarlyData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransmissionContract {
    pub mode: TransmissionMode,
    pub replay: MessageReplaySemantics,
    pub has_deduplication_identity: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransmissionIssue {
    NonReplayableEarlyData,
    MissingDeduplicationIdentity,
}

pub fn validate_transmission(
    contract: &TransmissionContract,
) -> Result<(), Vec<TransmissionIssue>> {
    let mut issues = Vec::new();

    if contract.mode == TransmissionMode::ReplayableEarlyData {
        match &contract.replay {
            MessageReplaySemantics::Idempotent => {}
            MessageReplaySemantics::Deduplicated { .. }
                if contract.has_deduplication_identity => {}
            MessageReplaySemantics::Deduplicated { .. } => {
                issues.push(
                    TransmissionIssue::MissingDeduplicationIdentity,
                );
            }
            MessageReplaySemantics::NonReplayable => {
                issues.push(TransmissionIssue::NonReplayableEarlyData);
            }
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerIdentityState {
    Verified,
    RotatedNeedsVerification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionMigrationAction {
    Continue,
    ReverifyPeer,
}

pub fn migration_action(
    identity: PeerIdentityState,
) -> ConnectionMigrationAction {
    match identity {
        PeerIdentityState::Verified => ConnectionMigrationAction::Continue,
        PeerIdentityState::RotatedNeedsVerification => {
            ConnectionMigrationAction::ReverifyPeer
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_replayable_mutation_cannot_use_early_data() {
        let contract = TransmissionContract {
            mode: TransmissionMode::ReplayableEarlyData,
            replay: MessageReplaySemantics::NonReplayable,
            has_deduplication_identity: false,
        };

        assert_eq!(
            validate_transmission(&contract),
            Err(vec![TransmissionIssue::NonReplayableEarlyData])
        );
    }

    #[test]
    fn deduplicated_early_data_requires_identity() {
        let contract = TransmissionContract {
            mode: TransmissionMode::ReplayableEarlyData,
            replay: MessageReplaySemantics::Deduplicated {
                identity_domain: "message-send".into(),
            },
            has_deduplication_identity: true,
        };

        assert!(validate_transmission(&contract).is_ok());
    }
}
