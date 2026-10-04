use g0::{
    bootstrap_compiler::{compiler_document, compiler_limits},
    execution::Executor,
    gir::{IntegerType, SemanticType},
    value::Value,
};
fn integer(min: i128, max: i128) -> SemanticType {
    SemanticType::Integer(IntegerType { min, max })
}
fn list(types: &[SemanticType]) -> Vec<u8> {
    let mut bytes = (types.len() as u32).to_le_bytes().to_vec();
    for (i, t) in types.iter().enumerate() {
        bytes.extend_from_slice(&(i as u16).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend(g0::graph_binary::encode_semantic_type(t).unwrap());
    }
    bytes
}
fn check(operation: &[u8], inputs: &[SemanticType], outputs: &[SemanticType]) -> bool {
    let mut bytes = operation.to_vec();
    let input_at = bytes.len();
    bytes.extend(list(inputs));
    let output_at = bytes.len();
    bytes.extend(list(outputs));
    let effects_at = bytes.len();
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let caps_at = bytes.len();
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let descriptor = Value::Array(
        [
            1,
            0,
            0,
            input_at,
            output_at,
            effects_at,
            caps_at,
            bytes.len(),
        ]
        .into_iter()
        .map(|v| Value::Integer(v as i128))
        .collect::<Vec<_>>()
        .into(),
    );
    let program = compiler_document().validated_contract().unwrap();
    let result = Executor::new(&program, compiler_limits())
        .unwrap()
        .run_graph(
            "validator-node-operation",
            vec![Value::Bytes(bytes.into()), descriptor],
        )
        .unwrap();
    let [Value::Bool(valid)] = result.as_slice() else {
        panic!("validator Bool")
    };
    *valid
}
#[test]
fn operation_validator_checks_boolean_shapes_and_constant_ranges() {
    assert!(check(
        &[22],
        &[SemanticType::Bool, SemanticType::Bool],
        &[SemanticType::Bool]
    ));
    assert!(!check(&[22], &[SemanticType::Bool], &[SemanticType::Bool]));
    assert!(!check(
        &[22],
        &[SemanticType::Bool, integer(0, 1)],
        &[SemanticType::Bool]
    ));
    let mut constant = vec![0, 1];
    constant.extend_from_slice(&42i128.to_le_bytes());
    assert!(check(&constant, &[], &[integer(-100, 100)]));
    assert!(!check(&constant, &[], &[integer(0, 41)]));
    assert!(!check(&constant, &[], &[SemanticType::Bool]));
}

#[test]
fn operation_validator_proves_integer_arithmetic_and_rejects_overflow() {
    assert!(check(
        &[1],
        &[integer(-2, 3), integer(4, 7)],
        &[integer(2, 10)]
    ));
    assert!(!check(
        &[1],
        &[integer(-2, 3), integer(4, 7)],
        &[integer(3, 10)]
    ));
    assert!(!check(
        &[1],
        &[integer(i128::MAX, i128::MAX), integer(1, 1)],
        &[integer(i128::MIN, i128::MAX)]
    ));
    assert!(check(
        &[2],
        &[integer(-2, 3), integer(4, 7)],
        &[integer(-9, -1)]
    ));
    assert!(check(
        &[3],
        &[integer(-2, 3), integer(-4, 7)],
        &[integer(-14, 21)]
    ));
    assert!(!check(
        &[3],
        &[integer(i128::MIN, i128::MIN), integer(-1, -1)],
        &[integer(i128::MIN, i128::MAX)]
    ));
    assert!(check(
        &[17],
        &[integer(-2, 3), integer(-2, 3)],
        &[SemanticType::Bool]
    ));
    assert!(!check(
        &[17],
        &[integer(-2, 3), integer(-2, 4)],
        &[SemanticType::Bool]
    ));
    assert!(check(
        &[28],
        &[integer(i128::MIN, i128::MAX)],
        &[integer(0, 255)]
    ));
    let result = SemanticType::Result(
        Box::new(integer(i128::MIN, i128::MAX)),
        Box::new(SemanticType::Bool),
    );
    assert!(check(&[70], &[integer(0, 1), integer(0, 1)], &[result]));
    assert!(!check(
        &[70],
        &[integer(0, 1), integer(0, 1)],
        &[SemanticType::Result(
            Box::new(integer(0, 2)),
            Box::new(SemanticType::Bool)
        )]
    ));
}

#[test]
fn operation_validator_checks_collections_text_and_result_contracts() {
    let byte = integer(0, 255);
    let full = integer(i128::MIN, i128::MAX);
    let option = SemanticType::Option(Box::new(byte.clone()));
    let slice = SemanticType::Slice(Box::new(byte.clone()));
    assert!(check(
        &[30],
        &[byte.clone(), integer(1, 10)],
        &[SemanticType::Array(Box::new(byte.clone()), 2)]
    ));
    assert!(!check(
        &[30],
        std::slice::from_ref(&byte),
        &[SemanticType::Array(Box::new(byte.clone()), 2)]
    ));
    assert!(check(
        &[31],
        &[SemanticType::Bytes, full.clone()],
        &[option]
    ));
    assert!(!check(
        &[31],
        &[SemanticType::Bytes, full.clone()],
        &[SemanticType::Option(Box::new(integer(0, 254)))]
    ));
    assert!(check(
        &[32],
        std::slice::from_ref(&slice),
        &[integer(0, u64::MAX as i128)]
    ));
    assert!(!check(&[32], std::slice::from_ref(&slice), &[integer(0, 255)]));
    assert!(check(
        &[48],
        &[
            SemanticType::Slice(Box::new(SemanticType::Text)),
            SemanticType::Text
        ],
        &[SemanticType::Text]
    ));
    assert!(!check(
        &[48],
        &[slice.clone(), SemanticType::Text],
        &[SemanticType::Text]
    ));
    assert!(check(
        &[66],
        &[SemanticType::Bytes, full.clone(), full.clone()],
        &[SemanticType::Bytes]
    ));
    assert!(check(
        &[67],
        &[
            slice.clone(),
            SemanticType::Array(Box::new(byte.clone()), 4)
        ],
        std::slice::from_ref(&slice)
    ));
    assert!(check(
        &[68],
        &[integer(0, 10)],
        &[SemanticType::Slice(Box::new(integer(0, 9)))]
    ));
    assert!(!check(
        &[68],
        &[integer(-1, 10)],
        &[SemanticType::Slice(Box::new(integer(0, 9)))]
    ));
    assert!(check(&[69], &[slice], &[SemanticType::Bytes]));
    assert!(!check(
        &[69],
        &[SemanticType::Slice(Box::new(integer(0, 256)))],
        &[SemanticType::Bytes]
    ));
    assert!(check(
        &[36],
        &[SemanticType::Bytes],
        &[SemanticType::Result(
            Box::new(SemanticType::Text),
            Box::new(SemanticType::Bytes)
        )]
    ));
    assert!(check(
        &[42],
        std::slice::from_ref(&byte),
        &[SemanticType::Option(Box::new(full.clone()))]
    ));
    assert!(check(
        &[45],
        &[SemanticType::Bool],
        &[SemanticType::Result(
            Box::new(full.clone()),
            Box::new(SemanticType::Bool)
        )]
    ));
    assert!(check(
        &[46],
        &[SemanticType::Option(Box::new(byte)), full.clone()],
        &[full]
    ));
}

#[test]
fn operation_validator_division_remainder_and_truncation_cover_signed_extremes() {
    assert!(check(
        &[26],
        &[integer(-10, 10), integer(-2, 2)],
        &[integer(-10, 10)]
    ));
    assert!(!check(
        &[26],
        &[integer(-10, 10), integer(-2, 2)],
        &[integer(-9, 10)]
    ));
    assert!(!check(
        &[26],
        &[integer(i128::MIN, i128::MIN), integer(-1, -1)],
        &[integer(i128::MIN, i128::MAX)]
    ));
    assert!(check(
        &[26],
        &[integer(i128::MIN, i128::MIN), integer(-1, 1)],
        &[integer(i128::MIN, i128::MIN)]
    ));
    assert!(!check(
        &[26],
        &[integer(0, 100), integer(0, 0)],
        &[integer(i128::MIN, i128::MAX)]
    ));
    assert!(check(
        &[27],
        &[integer(-10, 10), integer(-3, 3)],
        &[integer(-2, 2)]
    ));
    assert!(!check(
        &[27],
        &[integer(-10, 10), integer(-3, 3)],
        &[integer(-1, 2)]
    ));
    assert!(check(
        &[27],
        &[integer(1, 10), integer(i128::MIN, i128::MIN)],
        &[integer(0, i128::MAX)]
    ));
    assert!(!check(
        &[27],
        &[integer(i128::MIN, i128::MIN), integer(-1, -1)],
        &[integer(i128::MIN, i128::MAX)]
    ));
    assert!(check(
        &[29, 64, 0, 0],
        &[integer(i128::MIN, i128::MAX)],
        &[integer(0, u64::MAX as i128)]
    ));
    assert!(check(
        &[29, 64, 0, 1],
        &[integer(i128::MIN, i128::MAX)],
        &[integer(i64::MIN as i128, i64::MAX as i128)]
    ));
    assert!(!check(&[29, 0, 0, 0], &[integer(0, 255)], &[integer(0, 0)]));
    assert!(!check(
        &[29, 8, 0, 0],
        &[integer(0, 255)],
        &[integer(0, 256)]
    ));
}
