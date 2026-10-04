#[path = "support/program_fixture.rs"]
mod fixture;
use g0::execution::ExecutionLimits;
use g0::runtime_resources::{RegionArena, RegionError, TaskError, TaskGroup};
use g0::value::Value;
use std::sync::Arc;

#[test]
fn region_handles_enforce_owner_lifetime_and_allocation_budget() {
    let mut arena = RegionArena::new(8).unwrap();
    let region = arena.create_region(None, 8, false).unwrap();
    let handle = arena.allocate(region, vec![1, 2, 3]).unwrap();
    assert_eq!(&*arena.borrow(&handle).unwrap(), &[1, 2, 3]);
    arena.write(&handle, 1, &[7]).unwrap();
    assert_eq!(&*arena.borrow(&handle).unwrap(), &[1, 7, 3]);
    assert_eq!(
        arena.allocate(region, vec![0; 6]).unwrap_err(),
        RegionError::Budget
    );
    assert_eq!(arena.write(&handle, 3, &[9]), Err(RegionError::Bounds));
    let foreign = RegionArena::new(8).unwrap();
    assert_eq!(
        foreign.borrow(&handle).unwrap_err(),
        RegionError::ForeignHandle
    );
    arena.close_region(region).unwrap();
    assert_eq!(arena.borrow(&handle).unwrap_err(), RegionError::Closed);
}

#[test]
fn parent_closure_releases_children_and_secret_storage_cannot_escape() {
    let mut arena = RegionArena::new(32).unwrap();
    let parent = arena.create_region(None, 16, true).unwrap();
    let child = arena.create_region(Some(parent), 16, false).unwrap();
    let handle = arena.allocate(child, vec![42; 8]).unwrap();
    assert!(arena.is_secret(child).unwrap());
    assert_eq!(arena.take(handle).unwrap_err(), RegionError::SecretExport);
    let handle = arena.allocate(child, vec![7; 8]).unwrap();
    arena.close_region(parent).unwrap();
    assert_eq!(arena.bytes_used(), 0);
    assert_eq!(arena.borrow(&handle).unwrap_err(), RegionError::Closed);
    assert_eq!(
        arena.allocate(child, vec![]).unwrap_err(),
        RegionError::Closed
    );
}

#[test]
fn tasks_reserve_parent_budgets_and_share_cancellation() {
    let program = Arc::new(fixture::call_program().validated_contract().unwrap());
    let limits = ExecutionLimits {
        max_steps: 20,
        max_value_bytes: 4096,
        max_call_depth: 8,
    };
    let mut tasks = TaskGroup::new(limits, Default::default());
    let child = tasks
        .spawn(
            program.clone(),
            "main".into(),
            vec![],
            Default::default(),
            limits,
        )
        .unwrap();
    assert_eq!(tasks.join(child).unwrap(), vec![Value::Integer(42)]);
    assert!(matches!(
        tasks.spawn(
            program.clone(),
            "main".into(),
            vec![],
            Default::default(),
            limits
        ),
        Err(TaskError::Budget)
    ));
    let mut tasks = TaskGroup::new(limits, Default::default());
    tasks.cancel();
    assert!(matches!(
        tasks.spawn(program, "main".into(), vec![], Default::default(), limits),
        Err(TaskError::Cancelled)
    ));
}

#[test]
fn child_tasks_cannot_invent_capabilities() {
    use g0::gir::{Capability, CapabilityClass};
    let program = Arc::new(fixture::call_program().validated_contract().unwrap());
    let mut tasks = TaskGroup::new(ExecutionLimits::default(), Default::default());
    let cap = Capability::new(CapabilityClass::Network, "connect", "service", "tenant");
    assert!(matches!(
        tasks.spawn(
            program,
            "main".into(),
            vec![],
            [cap].into(),
            ExecutionLimits::default()
        ),
        Err(TaskError::Authority)
    ));
}

#[test]
fn independent_native_tasks_join_with_their_typed_results() {
    let mut tasks = TaskGroup::new(
        ExecutionLimits {
            max_steps: 100,
            max_value_bytes: 65536,
            max_call_depth: 8,
        },
        Default::default(),
    );
    let limits = ExecutionLimits {
        max_steps: 30,
        max_value_bytes: 32768,
        max_call_depth: 8,
    };
    let a = tasks
        .spawn(
            Arc::new(fixture::select_program().validated_contract().unwrap()),
            "main".into(),
            vec![],
            Default::default(),
            limits,
        )
        .unwrap();
    let b = tasks
        .spawn(
            Arc::new(fixture::loop_program().validated_contract().unwrap()),
            "main".into(),
            vec![],
            Default::default(),
            limits,
        )
        .unwrap();
    assert_eq!(tasks.join(a).unwrap(), vec![Value::Integer(42)]);
    assert_eq!(tasks.join(b).unwrap(), vec![Value::Integer(0)]);
}
