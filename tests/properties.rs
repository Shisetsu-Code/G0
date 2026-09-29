use g0::authority::{
    authorize, Action, AuthorizationDecision, PolicyExpr, PolicyRule,
    PolicySet, Principal, ResourceContext,
};
use g0::evolution::{initial_population, EvolutionConfig};
use g0::gir::{IntegerType, ParameterPolicy};
use g0::memory::{choose_integer_width, IntegerWidth};
use g0::tuning::{OptimizationProfile, ParameterName, SearchParameter};

#[test]
fn integer_width_selection_contains_all_small_ranges() {
    for min in -128_i128..=128 {
        for max in min..=128 {
            let range = IntegerType::new(min, max).unwrap();
            let width = choose_integer_width(&range);
            assert!(width_contains(width, min));
            assert!(width_contains(width, max));
        }
    }
}

#[test]
fn default_deny_holds_for_many_principal_resource_pairs() {
    for principal_index in 0..128 {
        for resource_index in 0..32 {
            let principal =
                Principal::new(format!("user-{principal_index}"), "tenant-a");
            let resource = ResourceContext::new(
                "Message",
                format!("message-{resource_index}"),
                "tenant-a",
            );

            assert_eq!(
                authorize(
                    &principal,
                    &resource,
                    &PolicySet::default(),
                    &Action::read(),
                    false,
                ),
                AuthorizationDecision::Deny(
                    g0::authority::DenyReason::NoPolicy
                )
            );
        }
    }
}

#[test]
fn owner_policy_never_authorizes_non_owner() {
    let policy = PolicySet {
        rules: vec![PolicyRule {
            action: Action::read(),
            allow_if: PolicyExpr::PrincipalOwnsResource,
        }],
    };

    for owner_index in 0..64 {
        let owner =
            Principal::new(format!("owner-{owner_index}"), "tenant-a");
        let mut resource =
            ResourceContext::new("Message", "message", "tenant-a");
        resource.owner = Some(owner.id.clone());

        for other_index in 0..64 {
            let other =
                Principal::new(format!("other-{other_index}"), "tenant-a");
            assert!(!authorize(
                &other,
                &resource,
                &policy,
                &Action::read(),
                false,
            )
            .allowed());
        }
    }
}

#[test]
fn evolutionary_search_never_mutates_fixed_parameters_across_many_seeds() {
    let fixed_name = ParameterName::new("security.floor");
    let profile = OptimizationProfile {
        parameters: vec![
            SearchParameter {
                name: fixed_name.clone(),
                policy: ParameterPolicy::Fixed(7),
            },
            SearchParameter {
                name: ParameterName::new("workers"),
                policy: ParameterPolicy::Bounded { min: 1, max: 64 },
            },
        ],
        ..OptimizationProfile::default()
    };

    for seed in 0..256_u64 {
        let population =
            initial_population(&profile, EvolutionConfig::default(), seed)
                .unwrap();
        assert!(population.iter().all(|genome| {
            genome.parameters[&fixed_name] == 7
        }));
    }
}

#[test]
fn bounded_optimizer_values_never_escape_range() {
    let workers = ParameterName::new("workers");
    let profile = OptimizationProfile {
        parameters: vec![SearchParameter {
            name: workers.clone(),
            policy: ParameterPolicy::Bounded { min: 2, max: 24 },
        }],
        ..OptimizationProfile::default()
    };

    for seed in 0..256_u64 {
        let population =
            initial_population(&profile, EvolutionConfig::default(), seed)
                .unwrap();
        assert!(population.iter().all(|genome| {
            let value = genome.parameters[&workers];
            (2..=24).contains(&value)
        }));
    }
}

fn width_contains(width: IntegerWidth, value: i128) -> bool {
    match width {
        IntegerWidth::U8 => (0..=u8::MAX as i128).contains(&value),
        IntegerWidth::U16 => (0..=u16::MAX as i128).contains(&value),
        IntegerWidth::U32 => (0..=u32::MAX as i128).contains(&value),
        IntegerWidth::U64 => value >= 0 && value <= u64::MAX as i128,
        IntegerWidth::U128 => value >= 0,
        IntegerWidth::I8 => (i8::MIN as i128..=i8::MAX as i128).contains(&value),
        IntegerWidth::I16 => {
            (i16::MIN as i128..=i16::MAX as i128).contains(&value)
        }
        IntegerWidth::I32 => {
            (i32::MIN as i128..=i32::MAX as i128).contains(&value)
        }
        IntegerWidth::I64 => {
            (i64::MIN as i128..=i64::MAX as i128).contains(&value)
        }
        IntegerWidth::I128 => true,
    }
}
