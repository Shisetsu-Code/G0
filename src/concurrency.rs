use std::collections::{BTreeMap, BTreeSet};

pub type TaskId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AccessKind {
    Read,
    Write,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum StateDiscipline {
    /// Single-owner mutable state. It may not be concurrently shared.
    Unique,
    /// Individual operations have atomic semantics.
    Atomic,
    /// Access is serialized by an exclusive capability.
    Exclusive,
    /// Mutable state is owned and serialized by an actor.
    Actor,
    /// Access occurs inside the storage/state transaction contract.
    Transactional,
    /// Writes use generation/version conflict detection.
    Versioned,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StateAccess {
    pub state: String,
    pub discipline: StateDiscipline,
    pub kind: AccessKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskAccess {
    pub task: TaskId,
    pub accesses: Vec<StateAccess>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ConcurrentRegion {
    /// Tasks in the same region are permitted to execute concurrently.
    pub tasks: Vec<TaskAccess>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConcurrencyIssue {
    DuplicateTask(TaskId),
    MixedDiscipline {
        state: String,
        disciplines: BTreeSet<StateDiscipline>,
    },
    ConcurrentUniqueMutation {
        state: String,
        tasks: BTreeSet<TaskId>,
    },
}

pub fn validate_concurrent_region(
    region: &ConcurrentRegion,
) -> Result<(), Vec<ConcurrencyIssue>> {
    let mut issues = Vec::new();
    let mut task_ids = BTreeSet::new();
    let mut by_state: BTreeMap<&str, Vec<(TaskId, &StateAccess)>> = BTreeMap::new();

    for task in &region.tasks {
        if !task_ids.insert(task.task) {
            issues.push(ConcurrencyIssue::DuplicateTask(task.task));
        }

        for access in &task.accesses {
            by_state
                .entry(access.state.as_str())
                .or_default()
                .push((task.task, access));
        }
    }

    for (state, accesses) in by_state {
        let disciplines: BTreeSet<StateDiscipline> = accesses
            .iter()
            .map(|(_, access)| access.discipline)
            .collect();

        if disciplines.len() > 1 {
            issues.push(ConcurrencyIssue::MixedDiscipline {
                state: state.to_owned(),
                disciplines,
            });
            continue;
        }

        let has_write = accesses
            .iter()
            .any(|(_, access)| access.kind == AccessKind::Write);
        let tasks: BTreeSet<TaskId> = accesses.iter().map(|(task, _)| *task).collect();

        if has_write
            && tasks.len() > 1
            && disciplines.contains(&StateDiscipline::Unique)
        {
            issues.push(ConcurrencyIssue::ConcurrentUniqueMutation {
                state: state.to_owned(),
                tasks,
            });
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

    fn access(state: &str, discipline: StateDiscipline, kind: AccessKind) -> StateAccess {
        StateAccess {
            state: state.into(),
            discipline,
            kind,
        }
    }

    #[test]
    fn independent_state_is_parallel_by_default() {
        let region = ConcurrentRegion {
            tasks: vec![
                TaskAccess {
                    task: 1,
                    accesses: vec![access(
                        "a",
                        StateDiscipline::Unique,
                        AccessKind::Write,
                    )],
                },
                TaskAccess {
                    task: 2,
                    accesses: vec![access(
                        "b",
                        StateDiscipline::Unique,
                        AccessKind::Write,
                    )],
                },
            ],
        };

        assert!(validate_concurrent_region(&region).is_ok());
    }

    #[test]
    fn raw_shared_unique_mutation_is_rejected() {
        let region = ConcurrentRegion {
            tasks: vec![
                TaskAccess {
                    task: 1,
                    accesses: vec![access(
                        "balance",
                        StateDiscipline::Unique,
                        AccessKind::Write,
                    )],
                },
                TaskAccess {
                    task: 2,
                    accesses: vec![access(
                        "balance",
                        StateDiscipline::Unique,
                        AccessKind::Read,
                    )],
                },
            ],
        };

        let issues = validate_concurrent_region(&region).unwrap_err();
        assert!(issues.iter().any(|issue| {
            matches!(
                issue,
                ConcurrencyIssue::ConcurrentUniqueMutation { state, .. }
                if state == "balance"
            )
        }));
    }

    #[test]
    fn transactional_shared_mutation_is_structurally_safe() {
        let region = ConcurrentRegion {
            tasks: vec![
                TaskAccess {
                    task: 1,
                    accesses: vec![access(
                        "account",
                        StateDiscipline::Transactional,
                        AccessKind::Write,
                    )],
                },
                TaskAccess {
                    task: 2,
                    accesses: vec![access(
                        "account",
                        StateDiscipline::Transactional,
                        AccessKind::Write,
                    )],
                },
            ],
        };

        assert!(validate_concurrent_region(&region).is_ok());
    }

    #[test]
    fn mixing_atomic_and_raw_access_is_rejected() {
        let region = ConcurrentRegion {
            tasks: vec![
                TaskAccess {
                    task: 1,
                    accesses: vec![access(
                        "counter",
                        StateDiscipline::Atomic,
                        AccessKind::Write,
                    )],
                },
                TaskAccess {
                    task: 2,
                    accesses: vec![access(
                        "counter",
                        StateDiscipline::Unique,
                        AccessKind::Read,
                    )],
                },
            ],
        };

        let issues = validate_concurrent_region(&region).unwrap_err();
        assert!(issues.iter().any(|issue| {
            matches!(
                issue,
                ConcurrencyIssue::MixedDiscipline { state, .. }
                if state == "counter"
            )
        }));
    }
}
