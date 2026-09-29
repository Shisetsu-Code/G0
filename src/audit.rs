#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRecord {
    pub sequence: u64,
    pub previous_record_hash: Option<String>,
    pub record_hash: String,
    pub principal: String,
    pub action: String,
    pub resource: String,
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AuditLog {
    pub records: Vec<AuditRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuditIssue {
    SequenceMismatch {
        expected: u64,
        actual: u64,
    },
    GenesisHasPreviousHash,
    MissingPreviousHash(u64),
    PreviousHashMismatch(u64),
    EmptyRecordHash(u64),
    EmptyPrincipal(u64),
}

pub fn validate_audit_log(
    log: &AuditLog,
) -> Result<(), Vec<AuditIssue>> {
    let mut issues = Vec::new();
    let mut previous: Option<&AuditRecord> = None;

    for (index, record) in log.records.iter().enumerate() {
        let expected_sequence = index as u64;
        if record.sequence != expected_sequence {
            issues.push(AuditIssue::SequenceMismatch {
                expected: expected_sequence,
                actual: record.sequence,
            });
        }

        if record.record_hash.is_empty() {
            issues.push(AuditIssue::EmptyRecordHash(record.sequence));
        }
        if record.principal.is_empty() {
            issues.push(AuditIssue::EmptyPrincipal(record.sequence));
        }

        match previous {
            None => {
                if record.previous_record_hash.is_some() {
                    issues.push(AuditIssue::GenesisHasPreviousHash);
                }
            }
            Some(previous) => match &record.previous_record_hash {
                None => issues.push(AuditIssue::MissingPreviousHash(
                    record.sequence,
                )),
                Some(hash) if hash != &previous.record_hash => {
                    issues.push(AuditIssue::PreviousHashMismatch(
                        record.sequence,
                    ));
                }
                Some(_) => {}
            },
        }

        previous = Some(record);
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuditRequirement {
    Required,
    Optional,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditedAction {
    pub action: String,
    pub requirement: AuditRequirement,
}

pub fn action_requires_audit(action: &AuditedAction) -> bool {
    action.requirement == AuditRequirement::Required
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_chain_detects_removed_or_reordered_record() {
        let log = AuditLog {
            records: vec![
                AuditRecord {
                    sequence: 0,
                    previous_record_hash: None,
                    record_hash: "a".into(),
                    principal: "admin".into(),
                    action: "permission.change".into(),
                    resource: "User:1".into(),
                    scope: "tenant".into(),
                },
                AuditRecord {
                    sequence: 1,
                    previous_record_hash: Some("wrong".into()),
                    record_hash: "b".into(),
                    principal: "admin".into(),
                    action: "permission.change".into(),
                    resource: "User:2".into(),
                    scope: "tenant".into(),
                },
            ],
        };

        assert_eq!(
            validate_audit_log(&log),
            Err(vec![AuditIssue::PreviousHashMismatch(1)])
        );
    }
}
