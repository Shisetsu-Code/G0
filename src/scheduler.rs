use std::collections::{BTreeMap, BTreeSet};

pub type ScheduledTaskId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SchedulingClass {
    LatencyCritical,
    Interactive,
    Throughput,
    Background,
}

impl SchedulingClass {
    fn rank(self) -> u8 {
        match self {
            Self::LatencyCritical => 0,
            Self::Interactive => 1,
            Self::Throughput => 2,
            Self::Background => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParallelismPolicy {
    Auto,
    Serial,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Affinity {
    Any,
    CoreLocal(String),
    NumaNode(u32),
    Accelerator(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSpec {
    pub id: ScheduledTaskId,
    pub dependencies: BTreeSet<ScheduledTaskId>,
    pub class: SchedulingClass,
    pub parallelism: ParallelismPolicy,
    pub affinity: Affinity,
    pub estimated_cost_ns: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ScheduleGraph {
    pub tasks: Vec<TaskSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleIssue {
    DuplicateTask(ScheduledTaskId),
    UnknownDependency {
        task: ScheduledTaskId,
        dependency: ScheduledTaskId,
    },
    Cycle,
    ZeroParallelCapacity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulerProfile {
    pub parallel_capacity: usize,
    pub work_stealing: bool,
    pub numa_aware: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleWave {
    pub tasks: Vec<ScheduledTaskId>,
}

pub fn validate_schedule(
    graph: &ScheduleGraph,
    profile: &SchedulerProfile,
) -> Result<(), Vec<ScheduleIssue>> {
    let mut issues = Vec::new();
    if profile.parallel_capacity == 0 {
        issues.push(ScheduleIssue::ZeroParallelCapacity);
    }

    let mut ids = BTreeSet::new();
    for task in &graph.tasks {
        if !ids.insert(task.id) {
            issues.push(ScheduleIssue::DuplicateTask(task.id));
        }
    }

    for task in &graph.tasks {
        for dependency in &task.dependencies {
            if !ids.contains(dependency) {
                issues.push(ScheduleIssue::UnknownDependency {
                    task: task.id,
                    dependency: *dependency,
                });
            }
        }
    }

    if topological_ids(graph).is_none() {
        issues.push(ScheduleIssue::Cycle);
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn schedule_waves(
    graph: &ScheduleGraph,
    profile: &SchedulerProfile,
) -> Result<Vec<ScheduleWave>, Vec<ScheduleIssue>> {
    validate_schedule(graph, profile)?;

    let mut remaining: BTreeSet<ScheduledTaskId> =
        graph.tasks.iter().map(|task| task.id).collect();
    let mut completed = BTreeSet::new();
    let by_id: BTreeMap<ScheduledTaskId, &TaskSpec> =
        graph.tasks.iter().map(|task| (task.id, task)).collect();
    let mut waves = Vec::new();

    while !remaining.is_empty() {
        let mut ready: Vec<&TaskSpec> = remaining
            .iter()
            .filter_map(|id| by_id.get(id).copied())
            .filter(|task| task.dependencies.is_subset(&completed))
            .collect();

        ready.sort_by_key(|task| {
            (
                task.class.rank(),
                std::cmp::Reverse(task.estimated_cost_ns),
                task.id,
            )
        });

        let mut selected = Vec::new();
        let mut capacity = profile.parallel_capacity;

        for task in ready {
            if capacity == 0 {
                break;
            }
            selected.push(task.id);
            if task.parallelism == ParallelismPolicy::Serial {
                break;
            }
            capacity -= 1;
        }

        if selected.is_empty() {
            return Err(vec![ScheduleIssue::Cycle]);
        }

        for id in &selected {
            remaining.remove(id);
            completed.insert(*id);
        }
        waves.push(ScheduleWave { tasks: selected });
    }

    Ok(waves)
}

pub fn downstream_costs(
    graph: &ScheduleGraph,
) -> Result<BTreeMap<ScheduledTaskId, u128>, ScheduleIssue> {
    let order = topological_ids(graph).ok_or(ScheduleIssue::Cycle)?;
    let by_id: BTreeMap<ScheduledTaskId, &TaskSpec> =
        graph.tasks.iter().map(|task| (task.id, task)).collect();
    let mut children =
        BTreeMap::<ScheduledTaskId, Vec<ScheduledTaskId>>::new();

    for task in &graph.tasks {
        for dependency in &task.dependencies {
            children.entry(*dependency).or_default().push(task.id);
        }
    }

    let mut cost = BTreeMap::new();
    for id in order.into_iter().rev() {
        let child_max = children
            .get(&id)
            .into_iter()
            .flatten()
            .filter_map(|child| cost.get(child).copied())
            .max()
            .unwrap_or(0);
        cost.insert(
            id,
            child_max.saturating_add(by_id[&id].estimated_cost_ns as u128),
        );
    }

    Ok(cost)
}

fn topological_ids(graph: &ScheduleGraph) -> Option<Vec<ScheduledTaskId>> {
    let ids: BTreeSet<ScheduledTaskId> =
        graph.tasks.iter().map(|task| task.id).collect();
    if ids.len() != graph.tasks.len() {
        return None;
    }

    let mut indegree: BTreeMap<ScheduledTaskId, usize> =
        ids.iter().map(|id| (*id, 0)).collect();
    let mut children =
        BTreeMap::<ScheduledTaskId, Vec<ScheduledTaskId>>::new();

    for task in &graph.tasks {
        for dependency in &task.dependencies {
            if !ids.contains(dependency) {
                continue;
            }
            *indegree.entry(task.id).or_default() += 1;
            children.entry(*dependency).or_default().push(task.id);
        }
    }

    let mut ready: BTreeSet<ScheduledTaskId> = indegree
        .iter()
        .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
        .collect();
    let mut order = Vec::with_capacity(ids.len());

    while let Some(id) = ready.pop_first() {
        order.push(id);
        if let Some(next) = children.get(&id) {
            for child in next {
                let degree = indegree.get_mut(child).expect("known task");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert(*child);
                }
            }
        }
    }

    (order.len() == ids.len()).then_some(order)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(
        id: ScheduledTaskId,
        dependencies: &[ScheduledTaskId],
        class: SchedulingClass,
        cost: u64,
    ) -> TaskSpec {
        TaskSpec {
            id,
            dependencies: dependencies.iter().copied().collect(),
            class,
            parallelism: ParallelismPolicy::Auto,
            affinity: Affinity::Any,
            estimated_cost_ns: cost,
        }
    }

    #[test]
    fn independent_tasks_share_wave() {
        let graph = ScheduleGraph {
            tasks: vec![
                task(1, &[], SchedulingClass::Throughput, 100),
                task(2, &[], SchedulingClass::Throughput, 100),
                task(3, &[1, 2], SchedulingClass::LatencyCritical, 10),
            ],
        };
        let profile = SchedulerProfile {
            parallel_capacity: 8,
            work_stealing: true,
            numa_aware: true,
        };

        let waves = schedule_waves(&graph, &profile).unwrap();
        assert_eq!(waves[0].tasks.len(), 2);
        assert_eq!(waves[1].tasks, vec![3]);
    }

    #[test]
    fn critical_downstream_cost_is_computed() {
        let graph = ScheduleGraph {
            tasks: vec![
                task(1, &[], SchedulingClass::Throughput, 100),
                task(2, &[1], SchedulingClass::Throughput, 200),
                task(3, &[2], SchedulingClass::Throughput, 300),
            ],
        };

        let costs = downstream_costs(&graph).unwrap();
        assert_eq!(costs[&1], 600);
        assert_eq!(costs[&3], 300);
    }
}
