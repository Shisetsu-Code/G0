use g0::{
    bootstrap_compiler::compiler_document,
    execution::{ExecutionLimits, Executor},
    value::Value,
};

#[test]
fn signed_decoding_uses_bounded_work_for_all_i128_bits() {
    let program = compiler_document().validated_contract().unwrap();
    let mut values = vec![
        i128::MIN,
        i128::MIN + 1,
        i128::MAX,
        i128::MAX - 1,
        -1,
        0,
        1,
        -256,
        255,
    ];
    let mut bits = 0x6a09e667f3bcc909bb67ae8584caa73bu128;
    for _ in 0..64 {
        bits ^= bits << 13;
        bits ^= bits >> 7;
        bits ^= bits << 17;
        values.push(bits as i128);
    }
    for value in values {
        let mut bytes = vec![91, 92, 93];
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes.extend_from_slice(&[94, 95]);
        let mut executor = Executor::new(
            &program,
            ExecutionLimits {
                max_steps: 20,
                max_value_bytes: 2 * 1024 * 1024,
                max_call_depth: 128,
            },
        )
        .unwrap();
        assert_eq!(
            executor
                .run_graph(
                    "reader-i128",
                    vec![Value::Bytes(bytes.into()), Value::Integer(3)]
                )
                .unwrap(),
            vec![Value::Integer(value)],
            "{value}"
        );
        assert!(executor.steps_used() <= 20);
    }
}

#[test]
fn signed_decoding_rejects_incomplete_words() {
    let program = compiler_document().validated_contract().unwrap();
    for length in [0, 1, 14, 15] {
        let mut executor = Executor::new(&program, ExecutionLimits::default()).unwrap();
        assert!(
            executor
                .run_graph(
                    "reader-i128",
                    vec![Value::Bytes(vec![0; length].into()), Value::Integer(0)]
                )
                .is_err()
        );
    }
}
