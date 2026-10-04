use g0::{
    execution::{ExecutionLimits, Executor, RuntimeError},
    gir::*,
    native_aggregate_runtime::NativeContext,
    program::ProgramContract,
    value::Value,
};
use std::sync::Arc;

fn program() -> ProgramContract {
    let input = Port {
        id: 7,
        name: "word".into(),
        ty: SemanticType::Bytes,
    };
    let output = Port {
        id: 99,
        name: "signed".into(),
        ty: SemanticType::Integer(IntegerType {
            min: i128::MIN,
            max: i128::MAX,
        }),
    };
    let mut graph = Graph::new("decode-review");
    graph.inputs = vec![input.clone()];
    graph.outputs = vec![output.clone()];
    graph.nodes.push(Node {
        id: 42,
        operation: Operation::DecodeInteger128Le,
        inputs: vec![input],
        outputs: vec![output],
        effects: Default::default(),
        required_capabilities: Default::default(),
    });
    graph.edges = vec![
        Edge {
            from: SourceEndpoint::GraphInput(7),
            to: TargetEndpoint::NodeInput { node: 42, port: 7 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 42, port: 99 },
            to: TargetEndpoint::GraphOutput(99),
        },
    ];
    ProgramContract {
        graphs: vec![graph],
        entry_graph: Some("decode-review".into()),
        ..Default::default()
    }
}

#[test]
fn random_words_match_both_runtimes_and_keep_source_bytes_immutable() {
    let p = program();
    let mut interpreter = Executor::new(&p, ExecutionLimits::default()).unwrap();
    let mut native = NativeContext::new(p.clone(), ExecutionLimits::default()).unwrap();
    let mut bits = 0x9e3779b97f4a7c156a09e667f3bcc909u128;
    for _ in 0..512 {
        bits ^= bits << 13;
        bits ^= bits >> 7;
        bits ^= bits << 17;
        let bytes: Arc<[u8]> = bits.to_le_bytes().to_vec().into();
        let saved = bytes.clone();
        let input = Value::Bytes(bytes);
        let expected = Value::Integer(bits as i128);
        assert_eq!(
            interpreter
                .run_graph("decode-review", vec![input.clone()])
                .unwrap(),
            vec![expected.clone()]
        );
        let handle = native.insert_value(input.clone()).unwrap();
        let output = native.primitive(0, 0, &[handle]);
        assert_eq!(native.value(output), Some(&expected));
        assert_eq!(native.value(handle), Some(&input));
        assert_eq!(saved.as_ref(), bits.to_le_bytes().as_slice());
    }
    assert_eq!(native.error, None);
}

#[test]
fn all_lengths_and_protected_or_nonbytes_inputs_fail_closed() {
    let p = program();
    for length in 0..=32 {
        let input = Value::Bytes(vec![0xff; length].into());
        let result = Executor::new(&p, ExecutionLimits::default())
            .unwrap()
            .run_graph("decode-review", vec![input.clone()]);
        let mut native = NativeContext::new(p.clone(), ExecutionLimits::default()).unwrap();
        let handle = native.insert_value(input).unwrap();
        let output = native.primitive(0, 0, &[handle]);
        if length == 16 {
            assert_eq!(result.unwrap(), vec![Value::Integer(-1)]);
            assert_eq!(native.value(output), Some(&Value::Integer(-1)));
        } else {
            assert!(matches!(result, Err(RuntimeError::Bounds { .. })));
            assert_eq!(output, 0);
            assert!(matches!(native.error, Some(RuntimeError::Bounds { .. })));
        }
    }
    let bytes = Value::Bytes(vec![0; 16].into());
    for input in [
        Value::Secret(Arc::new(bytes.clone())),
        Value::Credential(Arc::new(bytes)),
        Value::Text("0123456789abcdef".into()),
        Value::Array(vec![Value::Integer(0); 16].into()),
    ] {
        assert!(
            Executor::new(&p, ExecutionLimits::default())
                .unwrap()
                .run_graph("decode-review", vec![input.clone()])
                .is_err()
        );
        let mut native = NativeContext::new(p.clone(), ExecutionLimits::default()).unwrap();
        let handle = native.insert_value(input).unwrap();
        assert_eq!(native.primitive(0, 0, &[handle]), 0);
        assert!(matches!(
            native.error,
            Some(RuntimeError::TypeMismatch { .. })
        ));
    }
}

#[test]
fn decoding_charges_native_results_and_obeys_interpreter_memory_limits() {
    let p = program();
    let input = Value::Bytes(vec![0; 16].into());
    let needed = input.resident_bytes().unwrap() + Value::Integer(0).resident_bytes().unwrap();
    for budget in [needed - 1, needed] {
        let limits = ExecutionLimits {
            max_value_bytes: budget,
            ..ExecutionLimits::default()
        };
        let mut native = NativeContext::new(p.clone(), limits).unwrap();
        let handle = native.insert_value(input.clone()).unwrap();
        let output = native.primitive(0, 0, &[handle]);
        if budget == needed {
            assert_eq!(native.value(output), Some(&Value::Integer(0)));
        } else {
            assert_eq!(output, 0);
            assert_eq!(native.error, Some(RuntimeError::MemoryLimit));
        }
    }
    // The interpreter also charges graph-frame metadata; its low budget must
    // reject before decoding, while a sufficient bounded budget succeeds.
    let limits = ExecutionLimits {
        max_value_bytes: needed,
        ..ExecutionLimits::default()
    };
    assert_eq!(
        Executor::new(&p, limits)
            .unwrap()
            .run_graph("decode-review", vec![input.clone()]),
        Err(RuntimeError::MemoryLimit)
    );
    let limits = ExecutionLimits {
        max_value_bytes: 4096,
        ..ExecutionLimits::default()
    };
    assert_eq!(
        Executor::new(&p, limits)
            .unwrap()
            .run_graph("decode-review", vec![input])
            .unwrap(),
        vec![Value::Integer(0)]
    );
}

#[test]
fn the_opcode_contract_requires_plain_bytes_full_i128_and_purity() {
    let p = program();
    let node = &p.graphs[0].nodes[0];
    for ty in [
        SemanticType::Secret(Box::new(SemanticType::Bytes)),
        SemanticType::Credential(Box::new(SemanticType::Bytes)),
        SemanticType::Array(
            Box::new(SemanticType::Integer(IntegerType { min: 0, max: 255 })),
            16,
        ),
    ] {
        let mut wrong = node.clone();
        wrong.inputs[0].ty = ty;
        assert!(g0::composite::validate_node(&wrong, None).is_err());
    }
    for range in [
        IntegerType {
            min: i128::MIN + 1,
            max: i128::MAX,
        },
        IntegerType {
            min: i128::MIN,
            max: i128::MAX - 1,
        },
    ] {
        let mut wrong = node.clone();
        wrong.outputs[0].ty = SemanticType::Integer(range);
        assert!(g0::composite::validate_node(&wrong, None).is_err());
    }
    let mut effects = p.clone();
    effects.graphs[0].nodes[0].effects.insert(Effect::Network);
    assert!(g0::gir_validate::validate(&effects.graphs[0]).is_err());
    assert!(NativeContext::new(effects, ExecutionLimits::default()).is_err());
    let mut capabilities = p.clone();
    capabilities.graphs[0].nodes[0]
        .required_capabilities
        .insert(Capability::new(
            CapabilityClass::Network,
            "read",
            "endpoint",
            "review",
        ));
    assert!(g0::gir_validate::validate(&capabilities.graphs[0]).is_err());
    assert!(NativeContext::new(capabilities, ExecutionLimits::default()).is_err());

    let bytes = g0::graph_binary::encode_graph(&p.graphs[0]).unwrap();
    for minor in [10, 11] {
        let mut supported = bytes.clone();
        supported[6..8].copy_from_slice(&(minor as u16).to_le_bytes());
        assert!(g0::graph_binary_decode::decode_graph(&supported).is_ok());
    }
    for minor in [0, 9, 12] {
        let mut unsupported = bytes.clone();
        unsupported[6..8].copy_from_slice(&(minor as u16).to_le_bytes());
        assert!(g0::graph_binary_decode::decode_graph(&unsupported).is_err());
    }
}
