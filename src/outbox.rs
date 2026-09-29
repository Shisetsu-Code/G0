use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EffectId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRecord {
    pub effect_id: EffectId,
    pub transaction_id: String,
    pub payload_hash: String,
    pub destination: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryState {
    Pending,
    Delivered,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxEntry {
    pub record: OutboxRecord,
    pub state: DeliveryState,
    pub attempts: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OutboxBatch {
    pub entries: Vec<OutboxEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutboxIssue {
    DuplicateEffectId(EffectId),
    EmptyTransactionId(EffectId),
    EmptyPayloadHash(EffectId),
    DeliveredWithoutAttempt(EffectId),
}

pub fn validate_outbox(
    batch: &OutboxBatch,
) -> Result<(), Vec<OutboxIssue>> {
    let mut issues = Vec::new();
    let mut ids = BTreeSet::new();

    for entry in &batch.entries {
        if !ids.insert(entry.record.effect_id.clone()) {
            issues.push(OutboxIssue::DuplicateEffectId(
                entry.record.effect_id.clone(),
            ));
        }
        if entry.record.transaction_id.is_empty() {
            issues.push(OutboxIssue::EmptyTransactionId(
                entry.record.effect_id.clone(),
            ));
        }
        if entry.record.payload_hash.is_empty() {
            issues.push(OutboxIssue::EmptyPayloadHash(
                entry.record.effect_id.clone(),
            ));
        }
        if entry.state == DeliveryState::Delivered && entry.attempts == 0 {
            issues.push(OutboxIssue::DeliveredWithoutAttempt(
                entry.record.effect_id.clone(),
            ));
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryDecision {
    Deliver,
    AlreadyDelivered,
}

pub fn delivery_decision(
    entry: &OutboxEntry,
) -> DeliveryDecision {
    match entry.state {
        DeliveryState::Pending => DeliveryDecision::Deliver,
        DeliveryState::Delivered => DeliveryDecision::AlreadyDelivered,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_identity_is_unique_in_outbox() {
        let record = OutboxRecord {
            effect_id: EffectId("event-1".into()),
            transaction_id: "tx-1".into(),
            payload_hash: "hash".into(),
            destination: "notifications".into(),
        };
        let batch = OutboxBatch {
            entries: vec![
                OutboxEntry {
                    record: record.clone(),
                    state: DeliveryState::Pending,
                    attempts: 0,
                },
                OutboxEntry {
                    record,
                    state: DeliveryState::Pending,
                    attempts: 0,
                },
            ],
        };

        assert!(matches!(
            validate_outbox(&batch),
            Err(issues) if matches!(
                issues.first(),
                Some(OutboxIssue::DuplicateEffectId(_))
            )
        ));
    }

    #[test]
    fn delivered_effect_is_not_sent_again() {
        let entry = OutboxEntry {
            record: OutboxRecord {
                effect_id: EffectId("event-1".into()),
                transaction_id: "tx-1".into(),
                payload_hash: "hash".into(),
                destination: "notifications".into(),
            },
            state: DeliveryState::Delivered,
            attempts: 1,
        };

        assert_eq!(
            delivery_decision(&entry),
            DeliveryDecision::AlreadyDelivered
        );
    }
}
