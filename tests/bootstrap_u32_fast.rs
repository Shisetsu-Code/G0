use g0::{
    bootstrap_compiler::compiler_document,
    execution::{ExecutionLimits, Executor},
    value::Value,
};

#[test]
fn unsigned_word_decoding_preserves_all_bits_with_bounded_work() {
    let program = compiler_document().validated_contract().unwrap();
    let mut cases = vec![0, 1, 255, 256, 65535, 65536, u32::MAX, u32::MAX - 1];
    for position in 0..4 {
        for byte in 0..=255u32 {
            cases.push(byte << (position * 8));
        }
    }
    let limits = ExecutionLimits {
        max_steps: 40,
        max_value_bytes: 1 << 20,
        ..ExecutionLimits::default()
    };
    let mut executor = Executor::new(&program, limits).unwrap();
    for value in cases {
        let mut bytes = vec![17, 18, 19];
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes.push(20);
        assert_eq!(
            executor
                .run_graph(
                    "reader-u32",
                    vec![Value::Bytes(bytes.into()), Value::Integer(3)]
                )
                .unwrap(),
            vec![Value::Integer(value as i128)]
        );
        assert!(executor.steps_used() <= 40);
    }
}

#[test]
fn unsigned_word_helper_retains_zero_padding_for_missing_bytes() {
    // The framing reader rejects truncation; this low-level word helper has
    // always zero-padded absent bytes. Preserve that contract for its callers.
    let program = compiler_document().validated_contract().unwrap();
    for (bytes, at, expected) in [
        (vec![], 0, 0),
        (vec![255], 0, 255),
        (vec![1, 2], 0, 513),
        (vec![7, 255], 1, 255),
        (vec![1, 2, 3], 9, 0),
    ] {
        let mut executor = Executor::new(&program, ExecutionLimits::default()).unwrap();
        assert_eq!(
            executor
                .run_graph(
                    "reader-u32",
                    vec![Value::Bytes(bytes.into()), Value::Integer(at)]
                )
                .unwrap(),
            vec![Value::Integer(expected)]
        );
    }
}
