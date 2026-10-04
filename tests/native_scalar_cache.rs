use g0::{
    execution::{ExecutionLimits, RuntimeError},
    native_aggregate_runtime::NativeContext,
    program_binary::ProgramDocument,
    value::Value,
};

fn context(limits: ExecutionLimits) -> NativeContext {
    let graph = g0::editor::GraphEditor::new().graph().clone();
    let document = ProgramDocument {
        entry_graph: graph.name.clone(),
        graphs: vec![graph],
        schemas: vec![],
    };
    NativeContext::new(document.validated_contract().unwrap(), limits).unwrap()
}

#[test]
fn repeated_small_scalars_share_storage_without_merging_value_types() {
    let mut context = context(ExecutionLimits::default());
    for value in [
        Value::Bool(false),
        Value::Bool(true),
        Value::Integer(-32),
        Value::Integer(-1),
        Value::Integer(0),
        Value::Integer(1),
        Value::Integer(255),
        Value::Integer(256),
        Value::Integer(1023),
    ] {
        let first = context.insert_value(value.clone()).unwrap();
        for _ in 0..100 {
            let next = context.insert_value(value.clone()).unwrap();
            assert_eq!(next, first);
            assert_eq!(context.value(next), Some(&value));
        }
    }
    let integer = context.insert_value(Value::Integer(1)).unwrap();
    let boolean = context.insert_value(Value::Bool(true)).unwrap();
    assert_ne!(integer, boolean);
}

#[test]
fn scalar_reuse_preserves_cumulative_memory_exhaustion() {
    let value = Value::Integer(1);
    let limits = ExecutionLimits {
        max_value_bytes: value.resident_bytes().unwrap() * 3,
        ..ExecutionLimits::default()
    };
    let mut context = context(limits);
    let first = context.insert_value(value.clone()).unwrap();
    for _ in 0..2 {
        assert_eq!(context.insert_value(value.clone()).unwrap(), first);
    }
    assert_eq!(context.insert_value(value), Err(RuntimeError::MemoryLimit));
    assert_eq!(
        context.insert_value(Value::Bool(false)),
        Err(RuntimeError::MemoryLimit)
    );
}

#[test]
fn uncached_i128_boundaries_keep_exact_values() {
    let mut context = context(ExecutionLimits::default());
    for value in [
        i128::MIN,
        i128::MIN + 1,
        -33,
        1024,
        i128::MAX - 1,
        i128::MAX,
    ] {
        for _ in 0..2 {
            let handle = context.insert_value(Value::Integer(value)).unwrap();
            assert_eq!(context.value(handle), Some(&Value::Integer(value)));
        }
    }
}
