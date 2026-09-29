#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceBudget {
    pub cpu_ns: Option<u128>,
    pub wall_ns: Option<u128>,
    pub memory_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CancellationScope {
    pub id: u32,
    pub parent: Option<u32>,
    pub budget: ResourceBudget,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeTask {
    pub id: u32,
    pub scope: u32,
    pub budget: ResourceBudget,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskPlan {
    pub scopes: Vec<CancellationScope>,
    pub tasks: Vec<RuntimeTask>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskPlanIssue {
    DuplicateScope(u32),
    DuplicateTask(u32),
    UnknownParent { scope: u32, parent: u32 },
    UnknownTaskScope { task: u32, scope: u32 },
    ScopeCycle(u32),
    Budget(TaskBudgetIssue),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskBudgetIssue {
    CpuExceedsParent(u32),
    WallExceedsParent(u32),
    MemoryExceedsParent(u32),
}

pub fn validate_child_budget(
    task: &RuntimeTask,
    parent: &CancellationScope,
) -> Result<(), Vec<TaskBudgetIssue>> {
    let mut issues = Vec::new();

    if exceeds_u128(task.budget.cpu_ns, parent.budget.cpu_ns) {
        issues.push(TaskBudgetIssue::CpuExceedsParent(task.id));
    }
    if exceeds_u128(task.budget.wall_ns, parent.budget.wall_ns) {
        issues.push(TaskBudgetIssue::WallExceedsParent(task.id));
    }
    if exceeds_u64(task.budget.memory_bytes, parent.budget.memory_bytes) {
        issues.push(TaskBudgetIssue::MemoryExceedsParent(task.id));
    }

    if issues.is_empty() { Ok(()) } else { Err(issues) }
}

pub fn validate_task_plan(plan: &TaskPlan) -> Result<(), Vec<TaskPlanIssue>> {
    use std::collections::{BTreeMap, BTreeSet};

    let mut issues = Vec::new();
    let mut scopes = BTreeMap::<u32, &CancellationScope>::new();

    for scope in &plan.scopes {
        if scopes.insert(scope.id, scope).is_some() {
            issues.push(TaskPlanIssue::DuplicateScope(scope.id));
        }
    }

    for scope in &plan.scopes {
        if let Some(parent) = scope.parent
            && !scopes.contains_key(&parent)
        {
            issues.push(TaskPlanIssue::UnknownParent {
                scope: scope.id,
                parent,
            });
        }

        let mut seen = BTreeSet::new();
        let mut current = scope.id;
        while seen.insert(current) {
            let Some(item) = scopes.get(&current).copied() else {
                break;
            };
            let Some(parent) = item.parent else {
                break;
            };
            current = parent;
        }
        if seen.contains(&current)
            && scopes.get(&current).and_then(|scope| scope.parent).is_some()
        {
            issues.push(TaskPlanIssue::ScopeCycle(scope.id));
        }
    }

    let mut task_ids = BTreeSet::new();
    for task in &plan.tasks {
        if !task_ids.insert(task.id) {
            issues.push(TaskPlanIssue::DuplicateTask(task.id));
        }
        let Some(scope) = scopes.get(&task.scope).copied() else {
            issues.push(TaskPlanIssue::UnknownTaskScope {
                task: task.id,
                scope: task.scope,
            });
            continue;
        };

        if let Err(budget_issues) = validate_child_budget(task, scope) {
            issues.extend(
                budget_issues
                    .into_iter()
                    .map(TaskPlanIssue::Budget),
            );
        }
    }

    if issues.is_empty() { Ok(()) } else { Err(issues) }
}

fn exceeds_u128(child: Option<u128>, parent: Option<u128>) -> bool {
    match (child, parent) {
        (Some(child), Some(parent)) => child > parent,
        (None, Some(_)) => true,
        _ => false,
    }
}

fn exceeds_u64(child: Option<u64>, parent: Option<u64>) -> bool {
    match (child, parent) {
        (Some(child), Some(parent)) => child > parent,
        (None, Some(_)) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_cannot_be_less_bounded_than_parent() {
        let parent = CancellationScope {
            id: 1,
            parent: None,
            budget: ResourceBudget {
                cpu_ns: Some(1000),
                wall_ns: Some(2000),
                memory_bytes: Some(4096),
            },
        };
        let task = RuntimeTask {
            id: 2,
            scope: 1,
            budget: ResourceBudget {
                cpu_ns: Some(2000),
                wall_ns: Some(1000),
                memory_bytes: Some(1024),
            },
        };

        assert_eq!(
            validate_child_budget(&task, &parent),
            Err(vec![TaskBudgetIssue::CpuExceedsParent(2)])
        );
    }
}
