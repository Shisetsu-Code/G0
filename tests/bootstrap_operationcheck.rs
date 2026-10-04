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
    assert!(!check(
        &[32],
        std::slice::from_ref(&slice),
        &[integer(0, 255)]
    ));
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

fn strings(tag: u8, names: &[&str], fields: bool) -> Vec<u8> {
    let mut out = vec![tag];
    for (i, name) in names.iter().enumerate() {
        if fields && i == 1 {
            out.extend_from_slice(&((names.len() - 1) as u32).to_le_bytes());
        }
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
    }
    if fields && names.len() == 1 {
        out.extend_from_slice(&0u32.to_le_bytes());
    }
    out
}
#[test]
fn operation_validator_checks_named_local_contracts_and_distinct_record_fields() {
    let record = SemanticType::Record("Payload".into());
    let variant = SemanticType::Variant("Payload".into());
    assert!(check(
        &strings(38, &["Payload", "left", "right"], true),
        &[integer(0, 10), SemanticType::Text],
        std::slice::from_ref(&record)
    ));
    assert!(!check(
        &strings(38, &["Payload", "left", "left"], true),
        &[integer(0, 10), SemanticType::Text],
        std::slice::from_ref(&record)
    ));
    assert!(!check(
        &strings(38, &["Payload", "left", "right"], true),
        &[integer(0, 10)],
        std::slice::from_ref(&record)
    ));
    assert!(!check(
        &strings(38, &["Other", "left"], true),
        &[integer(0, 10)],
        std::slice::from_ref(&record)
    ));
    assert!(check(
        &strings(39, &["left"], false),
        std::slice::from_ref(&record),
        &[integer(0, 10)]
    ));
    assert!(!check(
        &strings(39, &[""], false),
        std::slice::from_ref(&record),
        &[integer(0, 10)]
    ));
    assert!(check(
        &strings(40, &["Payload", "answer"], false),
        &[integer(0, 10)],
        std::slice::from_ref(&variant)
    ));
    assert!(!check(
        &strings(40, &["Other", "answer"], false),
        &[integer(0, 10)],
        std::slice::from_ref(&variant)
    ));
    assert!(!check(
        &strings(40, &["Payload", ""], false),
        &[integer(0, 10)],
        std::slice::from_ref(&variant)
    ));
    assert!(check(
        &strings(41, &["answer"], false),
        std::slice::from_ref(&variant),
        &[SemanticType::Option(Box::new(integer(0, 10)))]
    ));
    assert!(!check(
        &strings(41, &["answer"], false),
        &[record],
        &[integer(0, 10)]
    ));
}

#[test]
fn operation_validator_accepts_every_remaining_pure_primitive_tag() {
    for tag in 18..=21 {
        assert!(
            check(
                &[tag],
                &[integer(-1, 1), integer(-1, 1)],
                &[SemanticType::Bool]
            ),
            "tag {tag}"
        );
    }
    for tag in 23..=24 {
        assert!(
            check(
                &[tag],
                &[SemanticType::Bool, SemanticType::Bool],
                &[SemanticType::Bool]
            ),
            "tag {tag}"
        );
    }
    assert!(check(&[25], &[SemanticType::Bool], &[SemanticType::Bool]));
    for (tag, inputs, output) in [
        (
            33,
            vec![SemanticType::Text, SemanticType::Text],
            SemanticType::Text,
        ),
        (
            34,
            vec![SemanticType::Bytes, SemanticType::Bytes],
            SemanticType::Bytes,
        ),
        (35, vec![SemanticType::Text], SemanticType::Bytes),
        (37, vec![integer(i128::MIN, i128::MAX)], SemanticType::Text),
        (
            43,
            vec![],
            SemanticType::Option(Box::new(SemanticType::Bool)),
        ),
        (
            44,
            vec![integer(0, 1)],
            SemanticType::Result(Box::new(integer(0, 255)), Box::new(SemanticType::Text)),
        ),
        (
            71,
            vec![integer(0, 1), integer(0, 1)],
            SemanticType::Result(
                Box::new(integer(i128::MIN, i128::MAX)),
                Box::new(SemanticType::Bool),
            ),
        ),
        (
            72,
            vec![integer(0, 1), integer(0, 1)],
            SemanticType::Result(
                Box::new(integer(i128::MIN, i128::MAX)),
                Box::new(SemanticType::Bool),
            ),
        ),
        (
            73,
            vec![SemanticType::Result(
                Box::new(SemanticType::Text),
                Box::new(SemanticType::Bytes),
            )],
            SemanticType::Bool,
        ),
    ] {
        assert!(check(&[tag], &inputs, &[output]), "tag {tag}");
    }
    assert!(check(
        &strings(38, &["Payload"], true),
        &[],
        &[SemanticType::Record("Payload".into())]
    ));
    for tag in [4, 5, 6, 7, 47, 49, 65, 76] {
        assert!(
            !check(&[tag], &[], &[]),
            "unsupported tag {tag} must fail closed"
        );
    }
    assert!(!check(
        &[76],
        &[SemanticType::Result(
            Box::new(SemanticType::Text),
            Box::new(SemanticType::Bytes)
        )],
        &[SemanticType::Bool]
    ));
}

#[test]
fn integer_binary_decode_requires_bytes_and_full_range() {
    assert!(check(
        &[74],
        &[SemanticType::Bytes],
        &[integer(i128::MIN, i128::MAX)]
    ));
    assert!(!check(
        &[74],
        &[SemanticType::Text],
        &[integer(i128::MIN, i128::MAX)]
    ));
    assert!(!check(&[74], &[SemanticType::Bytes], &[integer(0, 255)]));
}

#[test]
fn unsigned_word_decode_requires_bytes_and_exact_u32_range() {
    assert!(check(
        &[75],
        &[SemanticType::Bytes],
        &[integer(0, u32::MAX as i128)]
    ));
    assert!(!check(
        &[75],
        &[SemanticType::Text],
        &[integer(0, u32::MAX as i128)]
    ));
    assert!(!check(&[75], &[SemanticType::Bytes], &[integer(0, 255)]));
    assert!(!check(
        &[75],
        &[SemanticType::Bytes],
        &[integer(-1, u32::MAX as i128)]
    ));
    assert!(!check(
        &[76],
        &[SemanticType::Bytes],
        &[integer(0, u32::MAX as i128)]
    ));
}

#[test]
fn operation_validator_integer_proofs_agree_with_gir_reference() {
    use g0::gir::*;
    let ranges = [
        (i128::MIN, i128::MIN),
        (-7, 4),
        (0, 0),
        (2, 11),
        (i128::MAX, i128::MAX),
    ];
    for (tag, op, a, b, out) in [
        (1, Operation::Add, ranges[1], ranges[3], (-5, 15)),
        (
            1,
            Operation::Add,
            ranges[4],
            ranges[3],
            (i128::MIN, i128::MAX),
        ),
        (
            2,
            Operation::Sub,
            ranges[0],
            ranges[3],
            (i128::MIN, i128::MAX),
        ),
        (3, Operation::Mul, ranges[1], ranges[3], (-77, 44)),
        (
            3,
            Operation::Mul,
            ranges[0],
            (-1, -1),
            (i128::MIN, i128::MAX),
        ),
        (
            26,
            Operation::Div,
            ranges[0],
            (-1, 1),
            (i128::MIN, i128::MIN),
        ),
        (
            26,
            Operation::Div,
            ranges[1],
            ranges[2],
            (i128::MIN, i128::MAX),
        ),
        (
            27,
            Operation::Rem,
            ranges[0],
            (-1, -1),
            (i128::MIN, i128::MAX),
        ),
        (
            27,
            Operation::Rem,
            ranges[1],
            ranges[0],
            (-i128::MAX, i128::MAX),
        ),
    ] {
        let inputs = vec![integer(a.0, a.1), integer(b.0, b.1)];
        let output = integer(out.0, out.1);
        let p = |id, ty| Port {
            id,
            name: format!("p{id}"),
            ty,
        };
        let mut graph = Graph::new("proof");
        graph.inputs = vec![p(0, inputs[0].clone()), p(1, inputs[1].clone())];
        graph.outputs = vec![p(0, output.clone())];
        graph.nodes = vec![Node {
            id: 1,
            operation: op,
            inputs: graph.inputs.clone(),
            outputs: graph.outputs.clone(),
            effects: Default::default(),
            required_capabilities: Default::default(),
        }];
        graph.edges = vec![
            Edge {
                from: SourceEndpoint::GraphInput(0),
                to: TargetEndpoint::NodeInput { node: 1, port: 0 },
            },
            Edge {
                from: SourceEndpoint::GraphInput(1),
                to: TargetEndpoint::NodeInput { node: 1, port: 1 },
            },
            Edge {
                from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
                to: TargetEndpoint::GraphOutput(0),
            },
        ];
        assert_eq!(
            check(&[tag], &inputs, &[output]),
            g0::gir_validate::validate(&graph).is_ok(),
            "tag {tag}, {a:?}, {b:?}, {out:?}"
        );
    }
}
