use g0::{
    bootstrap_compiler::compiler_document,
    execution::{ExecutionLimits, Executor},
    value::Value,
};
#[test]
fn integer_bounds_compare_extrema_and_offset_in_twenty_steps() {
    let program = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(
        &program,
        ExecutionLimits {
            max_steps: 20,
            max_value_bytes: 8 * 1024 * 1024,
            max_call_depth: 128,
        },
    )
    .unwrap();
    for (minimum, maximum) in [
        (i128::MIN, i128::MAX),
        (i128::MIN, i128::MIN),
        (i128::MAX, i128::MAX),
        (i128::MAX, i128::MIN),
        (-1, 0),
        (0, -1),
    ] {
        let mut bytes = vec![91, 92, 93];
        bytes.extend(minimum.to_le_bytes());
        bytes.extend(maximum.to_le_bytes());
        assert_eq!(
            executor
                .run_graph(
                    "reader-i128-bounds",
                    vec![Value::Bytes(bytes.into()), Value::Integer(3)]
                )
                .unwrap(),
            vec![Value::Bool(minimum <= maximum)]
        );
        assert!(executor.steps_used() <= 20);
    }
}
