use std::cmp::Ordering;
use std::collections::BTreeMap;

use crate::gir::ParameterPolicy;
use crate::tuning::{
    validate_candidate, Candidate, Objective, ObjectiveDirection,
    OptimizationProfile, ParameterName,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvolutionConfig {
    pub population_size: usize,
    pub elite_count: usize,
    pub mutation_divisor: i128,
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            population_size: 32,
            elite_count: 8,
            mutation_divisor: 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvolutionIssue {
    EmptyPopulation,
    NoValidCandidate,
    InvalidConfig,
    MissingObjectiveMetric(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Genome {
    pub parameters: BTreeMap<ParameterName, i128>,
}

pub fn initial_population(
    profile: &OptimizationProfile,
    config: EvolutionConfig,
    seed: u64,
) -> Result<Vec<Genome>, EvolutionIssue> {
    if config.population_size == 0
        || config.elite_count == 0
        || config.elite_count > config.population_size
        || config.mutation_divisor <= 0
    {
        return Err(EvolutionIssue::InvalidConfig);
    }

    let base = base_genome(profile);
    let mut rng = DeterministicRng::new(seed);
    let mut population = Vec::with_capacity(config.population_size);
    population.push(base.clone());

    while population.len() < config.population_size {
        population.push(mutate_genome(
            profile,
            &base,
            config.mutation_divisor,
            &mut rng,
        ));
    }

    Ok(population)
}

pub fn next_generation(
    profile: &OptimizationProfile,
    scored: &[Candidate],
    config: EvolutionConfig,
    seed: u64,
) -> Result<Vec<Genome>, EvolutionIssue> {
    if scored.is_empty() {
        return Err(EvolutionIssue::EmptyPopulation);
    }
    if config.population_size == 0
        || config.elite_count == 0
        || config.elite_count > config.population_size
        || config.mutation_divisor <= 0
    {
        return Err(EvolutionIssue::InvalidConfig);
    }

    let mut valid: Vec<&Candidate> = scored
        .iter()
        .filter(|candidate| validate_candidate(profile, candidate).is_ok())
        .collect();

    if valid.is_empty() {
        return Err(EvolutionIssue::NoValidCandidate);
    }

    let objectives = sorted_objectives(profile);
    for candidate in &valid {
        for objective in &objectives {
            if !candidate.metrics.contains_key(&objective.metric) {
                return Err(EvolutionIssue::MissingObjectiveMetric(
                    objective.metric.0.clone(),
                ));
            }
        }
    }

    valid.sort_by(|a, b| compare_candidates(a, b, &objectives));

    let elite_count = config.elite_count.min(valid.len());
    let elites: Vec<Genome> = valid
        .iter()
        .take(elite_count)
        .map(|candidate| Genome {
            parameters: candidate.parameters.clone(),
        })
        .collect();

    let mut next = elites.clone();
    let mut rng = DeterministicRng::new(seed);

    while next.len() < config.population_size {
        let parent_a = &elites[rng.index(elites.len())];
        let parent_b = &elites[rng.index(elites.len())];
        let crossed = crossover(profile, parent_a, parent_b, &mut rng);
        next.push(mutate_genome(
            profile,
            &crossed,
            config.mutation_divisor,
            &mut rng,
        ));
    }

    Ok(next)
}

fn base_genome(profile: &OptimizationProfile) -> Genome {
    let parameters = profile
        .parameters
        .iter()
        .map(|parameter| {
            let value = match parameter.policy {
                ParameterPolicy::Fixed(value) => value,
                ParameterPolicy::Bounded { min, max } => {
                    min.saturating_add(max.saturating_sub(min) / 2)
                }
                ParameterPolicy::Free => 0,
            };
            (parameter.name.clone(), value)
        })
        .collect();

    Genome { parameters }
}

fn mutate_genome(
    profile: &OptimizationProfile,
    parent: &Genome,
    mutation_divisor: i128,
    rng: &mut DeterministicRng,
) -> Genome {
    let mut parameters = parent.parameters.clone();

    for parameter in &profile.parameters {
        let current = parameters.get(&parameter.name).copied().unwrap_or(0);

        let next = match parameter.policy {
            ParameterPolicy::Fixed(value) => value,
            ParameterPolicy::Bounded { min, max } => {
                let span = max.saturating_sub(min);
                let step = (span / mutation_divisor).max(1);
                let direction = rng.trit();
                current
                    .saturating_add(step.saturating_mul(direction))
                    .clamp(min, max)
            }
            ParameterPolicy::Free => {
                let step = (current.abs() / mutation_divisor).max(1);
                current.saturating_add(step.saturating_mul(rng.trit()))
            }
        };

        parameters.insert(parameter.name.clone(), next);
    }

    Genome { parameters }
}

fn crossover(
    profile: &OptimizationProfile,
    a: &Genome,
    b: &Genome,
    rng: &mut DeterministicRng,
) -> Genome {
    let mut parameters = BTreeMap::new();

    for parameter in &profile.parameters {
        let value = match parameter.policy {
            ParameterPolicy::Fixed(value) => value,
            _ => {
                let source = if rng.bit() {
                    &a.parameters
                } else {
                    &b.parameters
                };
                source.get(&parameter.name).copied().unwrap_or(0)
            }
        };
        parameters.insert(parameter.name.clone(), value);
    }

    Genome { parameters }
}

fn sorted_objectives(profile: &OptimizationProfile) -> Vec<Objective> {
    let mut objectives = profile.objectives.clone();
    objectives.sort_by_key(|objective| objective.priority);
    objectives
}

fn compare_candidates(
    a: &Candidate,
    b: &Candidate,
    objectives: &[Objective],
) -> Ordering {
    for objective in objectives {
        let av = a.metrics[&objective.metric];
        let bv = b.metrics[&objective.metric];
        let ordering = match objective.direction {
            ObjectiveDirection::Minimize => av.cmp(&bv),
            ObjectiveDirection::Maximize => bv.cmp(&av),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }

    a.parameters.cmp(&b.parameters)
}

struct DeterministicRng {
    state: u64,
}

impl DeterministicRng {
    fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x9E37_79B9_7F4A_7C15,
        }
    }

    fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    fn bit(&mut self) -> bool {
        self.next() & 1 == 1
    }

    fn trit(&mut self) -> i128 {
        match self.next() % 3 {
            0 => -1,
            1 => 0,
            _ => 1,
        }
    }

    fn index(&mut self, len: usize) -> usize {
        (self.next() as usize) % len
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tuning::{
        HardConstraint, MetricName, Objective, SearchParameter,
    };

    fn profile() -> OptimizationProfile {
        OptimizationProfile {
            parameters: vec![
                SearchParameter {
                    name: ParameterName::new("security.floor"),
                    policy: ParameterPolicy::Fixed(3),
                },
                SearchParameter {
                    name: ParameterName::new("workers"),
                    policy: ParameterPolicy::Bounded { min: 1, max: 32 },
                },
                SearchParameter {
                    name: ParameterName::new("batch"),
                    policy: ParameterPolicy::Bounded { min: 1, max: 4096 },
                },
            ],
            constraints: vec![HardConstraint::Invariant(
                "security.high".into(),
            )],
            objectives: vec![
                Objective {
                    metric: MetricName::new("p99_ns"),
                    direction: ObjectiveDirection::Minimize,
                    priority: 0,
                },
                Objective {
                    metric: MetricName::new("throughput"),
                    direction: ObjectiveDirection::Maximize,
                    priority: 1,
                },
            ],
        }
    }

    #[test]
    fn initial_population_never_mutates_fixed_invariant_parameter() {
        let population =
            initial_population(&profile(), EvolutionConfig::default(), 7)
                .unwrap();

        assert!(population.iter().all(|genome| {
            genome.parameters[&ParameterName::new("security.floor")] == 3
        }));
    }

    #[test]
    fn population_generation_is_reproducible_for_same_seed() {
        let a =
            initial_population(&profile(), EvolutionConfig::default(), 42)
                .unwrap();
        let b =
            initial_population(&profile(), EvolutionConfig::default(), 42)
                .unwrap();

        assert_eq!(a, b);
    }

    #[test]
    fn invalid_fast_candidate_is_removed_before_selection() {
        let profile = profile();

        let mut insecure = Candidate::default();
        insecure.parameters = BTreeMap::from([
            (ParameterName::new("security.floor"), 3),
            (ParameterName::new("workers"), 32),
            (ParameterName::new("batch"), 4096),
        ]);
        insecure
            .metrics
            .insert(MetricName::new("p99_ns"), 1);
        insecure
            .metrics
            .insert(MetricName::new("throughput"), 1_000_000);

        let mut secure = insecure.clone();
        secure.passed_invariants.push("security.high".into());
        secure
            .metrics
            .insert(MetricName::new("p99_ns"), 100);

        let next = next_generation(
            &profile,
            &[insecure, secure],
            EvolutionConfig {
                population_size: 4,
                elite_count: 1,
                mutation_divisor: 8,
            },
            1,
        )
        .unwrap();

        assert_eq!(
            next[0].parameters[&ParameterName::new("security.floor")],
            3
        );
    }
}
