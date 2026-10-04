use g0::{
    bootstrap_compiler::compiler_document,
    execution::{ExecutionLimits, Executor},
    value::Value,
};

#[test]
fn common_collection_types_do_not_walk_unrelated_numeric_tags() {
    let program = compiler_document().validated_contract().unwrap();
    let limits = ExecutionLimits {
        max_steps: 120,
        max_value_bytes: 1 << 20,
        ..ExecutionLimits::default()
    };
    let mut executor = Executor::new(&program, limits).unwrap();
    for ty in [vec![7], vec![8], vec![10, 0], vec![14, 0], vec![15, 0, 0]] {
        let mut bytes = vec![17, 18, 19];
        bytes.extend_from_slice(&ty);
        assert_eq!(
            executor
                .run_graph(
                    "reader-type-layout",
                    vec![Value::Bytes(bytes.into()), Value::Integer(3)]
                )
                .unwrap(),
            vec![Value::Integer((3 + ty.len()) as i128)]
        );
        assert!(executor.steps_used() <= 120);
    }
}

#[test]
fn type_dispatch_preserves_less_frequent_payloads_and_unknown_tag_rejection() {
    let program = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&program, ExecutionLimits::default()).unwrap();
    for tag in 0..=24 {
        let mut bytes = vec![tag];
        match tag {
            1 => bytes.extend_from_slice(&[0; 32]),
            4 => bytes.extend_from_slice(&[0; 8]),
            5 => bytes.push(0),
            6 => bytes.extend_from_slice(&[0; 4]),
            9 | 11 => {
                bytes.extend_from_slice(&[0; 8]);
                bytes.push(0);
            }
            12 | 13 | 16 => {
                bytes.extend_from_slice(&1u32.to_le_bytes());
                bytes.push(b'x');
            }
            15 => bytes.extend_from_slice(&[0, 0]),
            10 | 14 | 17..=24 => bytes.push(0),
            _ => {}
        }
        let end = bytes.len();
        assert_eq!(
            executor
                .run_graph(
                    "reader-type-layout",
                    vec![Value::Bytes(bytes.into()), Value::Integer(0)]
                )
                .unwrap(),
            vec![Value::Integer(end as i128)],
            "tag {tag}"
        );
    }
    for tag in [25, 127, 255] {
        assert_eq!(
            executor
                .run_graph(
                    "reader-type-layout",
                    vec![Value::Bytes(vec![tag].into()), Value::Integer(0)]
                )
                .unwrap(),
            vec![Value::Integer(1_i128 << 48)]
        );
    }
}
