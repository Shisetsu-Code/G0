use g0::{
    execution::{ExecutionLimits, RuntimeError},
    gir::{Edge, IntegerType, Literal, Operation, SemanticType, SourceEndpoint, TargetEndpoint},
    native_aggregate_runtime::{
        g0_native_pack, g0_native_pack_check, g0_native_pack_item, NativeContext,
    },
    program_binary::ProgramDocument,
    value::Value,
};
use std::collections::BTreeSet;

fn context(limits: ExecutionLimits) -> NativeContext {
    let mut graph = g0::editor::GraphEditor::new().graph().clone();
    graph.nodes[0].operation = Operation::Const(Literal::Integer(1));
    let integer_type = SemanticType::Integer(IntegerType { min: 1, max: 1 });
    graph.nodes[0].outputs[0].ty = integer_type.clone();
    graph.outputs[0].ty = integer_type;
    let mut boolean = graph.nodes[0].clone();
    boolean.id = 2;
    boolean.operation = Operation::Const(Literal::Bool(true));
    boolean.outputs[0].ty = SemanticType::Bool;
    graph.nodes.push(boolean);
    let mut integer_output = graph.outputs[0].clone();
    integer_output.id = 1;
    integer_output.name = "second".into();
    graph.outputs.push(integer_output);
    let mut bool_output = graph.outputs[0].clone();
    bool_output.id = 2;
    bool_output.name = "boolean".into();
    bool_output.ty = SemanticType::Bool;
    graph.outputs.push(bool_output);
    graph.edges.extend([
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::GraphOutput(1),
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
            to: TargetEndpoint::GraphOutput(2),
        },
    ]);
    let document = ProgramDocument {
        entry_graph: graph.name.clone(),
        graphs: vec![graph],
        schemas: vec![],
    };
    NativeContext::new(document.validated_contract().unwrap(), limits).unwrap()
}

#[test]
fn every_cached_scalar_slot_is_distinct_and_stable() {
    let mut c = context(ExecutionLimits::default());
    let mut entries = Vec::new();
    for value in (-32..=1023)
        .map(Value::Integer)
        .chain([Value::Bool(false), Value::Bool(true)])
    {
        entries.push((c.insert_value(value.clone()).unwrap(), value));
    }
    assert_eq!(
        entries
            .iter()
            .map(|(h, _)| *h)
            .collect::<BTreeSet<_>>()
            .len(),
        1058
    );
    for (handle, value) in entries.into_iter().rev() {
        assert_eq!(c.insert_value(value.clone()).unwrap(), handle);
        assert_eq!(c.value(handle), Some(&value));
    }
    for value in [-33, 1024] {
        let a = c.insert_value(Value::Integer(value)).unwrap();
        let b = c.insert_value(Value::Integer(value)).unwrap();
        assert_ne!(a, b);
    }
}

#[test]
fn native_primitives_and_packs_preserve_scalar_types_and_sticky_failure() {
    let mut c = context(ExecutionLimits::default());
    let integer = c.insert_value(Value::Integer(1)).unwrap();
    let boolean = c.insert_value(Value::Bool(true)).unwrap();
    for _ in 0..16 {
        assert_eq!(c.primitive(0, 0, &[]), integer);
        assert_eq!(c.primitive(0, 1, &[]), boolean);
    }
    let handles = [integer, integer, boolean];
    unsafe {
        let pack = g0_native_pack(&mut c, handles.as_ptr(), 3);
        assert_ne!(pack, 0);
        assert!(c.value(pack).is_none());
        assert_eq!(g0_native_pack_check(&mut c, 0, u64::MAX, pack), pack);
        assert_eq!(g0_native_pack_item(&mut c, pack, 0), integer);
        assert_eq!(g0_native_pack_item(&mut c, pack, 1), integer);
        assert_eq!(g0_native_pack_item(&mut c, pack, 2), boolean);
        let wrong_handles = [boolean, integer, boolean];
        let wrong = g0_native_pack(&mut c, wrong_handles.as_ptr(), 3);
        assert_eq!(g0_native_pack_check(&mut c, 0, u64::MAX, wrong), 0);
        let failure = c.error.clone().unwrap();
        assert!(matches!(failure, RuntimeError::TypeMismatch { .. }));
        assert_eq!(c.insert_value(Value::Integer(1)), Err(failure.clone()));
        assert_eq!(c.primitive(0, 0, &[]), 0);
        assert_eq!(g0_native_pack_item(&mut c, pack, 0), 0);
        assert_eq!(c.error, Some(failure));
    }
}

#[test]
fn cached_primitive_outputs_still_exhaust_the_exact_logical_budget() {
    let bytes = Value::Integer(1).resident_bytes().unwrap();
    let mut c = context(ExecutionLimits {
        max_value_bytes: bytes * 4,
        ..ExecutionLimits::default()
    });
    let first = c.insert_value(Value::Integer(1)).unwrap();
    for _ in 0..3 {
        assert_eq!(c.primitive(0, 0, &[]), first);
    }
    assert_eq!(c.primitive(0, 0, &[]), 0);
    assert_eq!(c.error, Some(RuntimeError::MemoryLimit));
    assert_eq!(
        c.insert_value(Value::Integer(1)),
        Err(RuntimeError::MemoryLimit)
    );
}

#[test]
fn a_cached_result_does_not_bypass_step_or_cancellation_checks() {
    let mut c = context(ExecutionLimits {
        max_steps: 1,
        ..ExecutionLimits::default()
    });
    let cached = c.insert_value(Value::Integer(1)).unwrap();
    assert_eq!(c.primitive(0, 0, &[]), cached);
    assert_eq!(c.primitive(0, 0, &[]), 0);
    assert_eq!(c.error, Some(RuntimeError::StepLimit));
    assert_eq!(
        c.insert_value(Value::Integer(1)),
        Err(RuntimeError::StepLimit)
    );

    let mut c = context(ExecutionLimits::default());
    c.insert_value(Value::Integer(1)).unwrap();
    c.cancellation().cancel();
    assert_eq!(c.primitive(0, 0, &[]), 0);
    assert_eq!(c.error, Some(RuntimeError::Cancelled));
    assert_eq!(
        c.insert_value(Value::Integer(1)),
        Err(RuntimeError::Cancelled)
    );
}
