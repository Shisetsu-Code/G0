#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationLevel {
    Serializable,
    Snapshot,
    ReadCommitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurabilityLevel {
    DurableBeforeAck,
    MemoryOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionContract {
    pub isolation: IsolationLevel,
    pub durability: DurabilityLevel,
    pub retry_on_conflict: bool,
    pub explicit_relaxation: bool,
}

impl TransactionContract {
    pub fn strict() -> Self {
        Self {
            isolation: IsolationLevel::Serializable,
            durability: DurabilityLevel::DurableBeforeAck,
            retry_on_conflict: true,
            explicit_relaxation: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionIssue {
    WeakerIsolationRequiresExplicitRelaxation,
    MemoryOnlyDurabilityRequiresExplicitRelaxation,
}

pub fn validate_transaction(
    contract: &TransactionContract,
) -> Result<(), Vec<TransactionIssue>> {
    let mut issues = Vec::new();

    if contract.isolation != IsolationLevel::Serializable
        && !contract.explicit_relaxation
    {
        issues.push(
            TransactionIssue::WeakerIsolationRequiresExplicitRelaxation,
        );
    }

    if contract.durability != DurabilityLevel::DurableBeforeAck
        && !contract.explicit_relaxation
    {
        issues.push(
            TransactionIssue::MemoryOnlyDurabilityRequiresExplicitRelaxation,
        );
    }

    if issues.is_empty() { Ok(()) } else { Err(issues) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionToken {
    pub resource: String,
    pub generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommitCheck {
    Ready,
    Conflict(Vec<String>),
}

pub fn check_versions(
    expected: &[VersionToken],
    current: &[VersionToken],
) -> CommitCheck {
    let mut conflicts = Vec::new();

    for wanted in expected {
        let actual = current
            .iter()
            .find(|token| token.resource == wanted.resource);
        if actual.map(|token| token.generation) != Some(wanted.generation) {
            conflicts.push(wanted.resource.clone());
        }
    }

    if conflicts.is_empty() {
        CommitCheck::Ready
    } else {
        CommitCheck::Conflict(conflicts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_transaction_is_default_contract() {
        let contract = TransactionContract::strict();
        assert_eq!(contract.isolation, IsolationLevel::Serializable);
        assert_eq!(
            contract.durability,
            DurabilityLevel::DurableBeforeAck
        );
        assert!(validate_transaction(&contract).is_ok());
    }

    #[test]
    fn stale_version_causes_conflict() {
        let expected = vec![VersionToken {
            resource: "Account:1".into(),
            generation: 10,
        }];
        let current = vec![VersionToken {
            resource: "Account:1".into(),
            generation: 11,
        }];

        assert_eq!(
            check_versions(&expected, &current),
            CommitCheck::Conflict(vec!["Account:1".into()])
        );
    }
}
