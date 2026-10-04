use g0::{
    execution::{ExecutionLimits, RuntimeError},
    gir::*,
    native_aggregate_runtime::*,
    program::ProgramContract,
    value::Value,
};
fn graph() -> Graph {
    let mut g = Graph::new("ports");
    g.inputs = vec![
        Port {
            id: 90,
            name: "bool".into(),
            ty: SemanticType::Bool,
        },
        Port {
            id: 3,
            name: "int".into(),
            ty: SemanticType::Integer(IntegerType { min: 4, max: 4 }),
        },
    ];
    g.outputs = g.inputs.clone();
    g.edges = g
        .inputs
        .iter()
        .map(|p| Edge {
            from: SourceEndpoint::GraphInput(p.id),
            to: TargetEndpoint::GraphOutput(p.id),
        })
        .collect();
    g
}
fn program(g: Graph) -> ProgramContract {
    ProgramContract {
        graphs: vec![g],
        ..Default::default()
    }
}
#[test]
fn cached_sparse_port_order_and_result_packs_preserve_handles() {
    let mut c = NativeContext::new(program(graph()), Default::default()).unwrap();
    let integer = c.insert_value(Value::Integer(4)).unwrap();
    let boolean = c.insert_value(Value::Bool(true)).unwrap();
    let args = [integer, boolean];
    unsafe {
        for _ in 0..10 {
            assert_eq!(g0_native_graph_enter(&mut c, 0, args.as_ptr(), 2), 1);
            g0_native_leave(&mut c);
        }
        let pack = g0_native_pack(&mut c, args.as_ptr(), 2);
        assert_eq!(g0_native_pack_check(&mut c, 0, u64::MAX, pack), pack);
        assert_eq!(g0_native_pack_item(&mut c, pack, 0), integer);
        assert_eq!(g0_native_pack_item(&mut c, pack, 1), boolean);
    }
    assert_eq!(c.error, None);
}
#[test]
fn cache_is_charged_once_and_tiny_quotas_fall_back_without_changing_frame_limits() {
    for memory in [144, 1024] {
        let mut c = NativeContext::new(
            program(Graph::new("empty")),
            ExecutionLimits {
                max_value_bytes: memory,
                ..Default::default()
            },
        )
        .unwrap();
        unsafe {
            for _ in 0..50 {
                assert_eq!(g0_native_enter(&mut c, 0), 1);
                g0_native_leave(&mut c);
            }
        }
        assert_eq!(c.error, None);
    }
    let mut c = NativeContext::new(
        program(Graph::new("empty")),
        ExecutionLimits {
            max_value_bytes: 143,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(unsafe { g0_native_enter(&mut c, 0) }, 0);
    assert_eq!(c.error, Some(RuntimeError::MemoryLimit));
}
#[test]
fn allocated_metadata_consumes_the_value_budget_after_frame_release() {
    let mut c = NativeContext::new(
        program(Graph::new("empty")),
        ExecutionLimits {
            max_value_bytes: 1024,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(unsafe { g0_native_enter(&mut c, 0) }, 1);
    unsafe { g0_native_leave(&mut c) };
    assert_eq!(
        c.insert_value(Value::Bytes(vec![0; 850].into())),
        Err(RuntimeError::MemoryLimit)
    );
}
#[test]
fn metadata_preparation_keeps_cancellation_depth_and_input_rejection() {
    let mut c = NativeContext::new(
        program(Graph::new("empty")),
        ExecutionLimits {
            max_call_depth: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(unsafe { g0_native_enter(&mut c, 0) }, 1);
    assert_eq!(unsafe { g0_native_enter(&mut c, 0) }, 0);
    assert_eq!(c.error, Some(RuntimeError::CallDepth));
    let mut c = NativeContext::new(program(Graph::new("empty")), Default::default()).unwrap();
    c.cancellation().cancel();
    assert_eq!(unsafe { g0_native_enter(&mut c, 0) }, 0);
    assert_eq!(c.error, Some(RuntimeError::Cancelled));
    let mut c = NativeContext::new(program(graph()), Default::default()).unwrap();
    let a = c.insert_value(Value::Integer(4)).unwrap();
    let b = c.insert_value(Value::Bool(true)).unwrap();
    let wrong = [b, a];
    assert_eq!(
        unsafe { g0_native_graph_enter(&mut c, 0, wrong.as_ptr(), 2) },
        0
    );
    assert!(matches!(
        c.error,
        Some(RuntimeError::TypeMismatch { node: None, .. })
    ));
}
#[test]
fn aggregate_native_stack_cap_still_rejects_large_nested_frames() {
    let mut g = Graph::new("large");
    g.inputs = (0..=u16::MAX)
        .map(|id| Port {
            id,
            name: format!("p{id}"),
            ty: SemanticType::Bool,
        })
        .collect();
    let mut c = NativeContext::new(program(g), Default::default()).unwrap();
    let mut entered = 0;
    while unsafe { g0_native_enter(&mut c, 0) } != 0 {
        entered += 1;
        assert!(entered < 9);
    }
    assert_eq!(entered, 7);
    assert_eq!(c.error, Some(RuntimeError::CallDepth));
}
