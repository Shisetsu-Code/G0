use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationStep {
    AddOptionalField { tag: u32 },
    BackfillField { tag: u32 },
    PromoteFieldToRequired { tag: u32 },
    RenameField { tag: u32 },
    RemoveField { tag: u32, allow_data_loss: bool },
    RebuildIndex { name: String },
    RotateStorageProtection { profile: String, to_version: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationMode {
    Online,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationPlan {
    pub from_version: u32,
    pub to_version: u32,
    pub mode: MigrationMode,
    pub steps: Vec<MigrationStep>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationIssue {
    VersionDoesNotIncrease,
    RequiredBeforeBackfill(u32),
    DestructiveRemovalNotExplicit(u32),
    DuplicateAdd(u32),
    BackfillBeforeAdd(u32),
    ProtectionVersionZero,
}

pub fn validate_migration(
    plan: &MigrationPlan,
) -> Result<(), Vec<MigrationIssue>> {
    let mut issues = Vec::new();

    if plan.to_version <= plan.from_version {
        issues.push(MigrationIssue::VersionDoesNotIncrease);
    }

    let mut added = BTreeSet::new();
    let mut backfilled = BTreeSet::new();

    for step in &plan.steps {
        match step {
            MigrationStep::AddOptionalField { tag } => {
                if !added.insert(*tag) {
                    issues.push(MigrationIssue::DuplicateAdd(*tag));
                }
            }
            MigrationStep::BackfillField { tag } => {
                if !added.contains(tag) {
                    issues.push(MigrationIssue::BackfillBeforeAdd(*tag));
                }
                backfilled.insert(*tag);
            }
            MigrationStep::PromoteFieldToRequired { tag } => {
                if !backfilled.contains(tag) {
                    issues.push(MigrationIssue::RequiredBeforeBackfill(*tag));
                }
            }
            MigrationStep::RemoveField {
                tag,
                allow_data_loss,
            } => {
                if !allow_data_loss {
                    issues.push(
                        MigrationIssue::DestructiveRemovalNotExplicit(*tag),
                    );
                }
            }
            MigrationStep::RotateStorageProtection { to_version, .. } => {
                if *to_version == 0 {
                    issues.push(MigrationIssue::ProtectionVersionZero);
                }
            }
            MigrationStep::RenameField { .. }
            | MigrationStep::RebuildIndex { .. } => {}
        }
    }

    if issues.is_empty() { Ok(()) } else { Err(issues) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_required_field_is_staged() {
        let plan = MigrationPlan {
            from_version: 1,
            to_version: 2,
            mode: MigrationMode::Online,
            steps: vec![
                MigrationStep::AddOptionalField { tag: 4 },
                MigrationStep::BackfillField { tag: 4 },
                MigrationStep::PromoteFieldToRequired { tag: 4 },
            ],
        };

        assert!(validate_migration(&plan).is_ok());
    }

    #[test]
    fn required_field_cannot_appear_before_backfill() {
        let plan = MigrationPlan {
            from_version: 1,
            to_version: 2,
            mode: MigrationMode::Online,
            steps: vec![
                MigrationStep::AddOptionalField { tag: 4 },
                MigrationStep::PromoteFieldToRequired { tag: 4 },
            ],
        };

        assert_eq!(
            validate_migration(&plan),
            Err(vec![MigrationIssue::RequiredBeforeBackfill(4)])
        );
    }

    #[test]
    fn destructive_remove_requires_explicit_data_loss_choice() {
        let plan = MigrationPlan {
            from_version: 2,
            to_version: 3,
            mode: MigrationMode::Offline,
            steps: vec![MigrationStep::RemoveField {
                tag: 7,
                allow_data_loss: false,
            }],
        };

        assert_eq!(
            validate_migration(&plan),
            Err(vec![MigrationIssue::DestructiveRemovalNotExplicit(7)])
        );
    }
}
