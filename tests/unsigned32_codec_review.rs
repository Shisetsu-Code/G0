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
        name: "unsigned".into(),
        ty: SemanticType::Integer(IntegerType {
            min: 0,
            max: u32::MAX as i128,
        }),
    };
    let mut graph = Graph::new("decode-unsigned-review");
    graph.inputs = vec![input.clone()];
    graph.outputs = vec![output.clone()];
    graph.nodes.push(Node {
        id: 42,
        operation: Operation::DecodeUnsigned32Le,
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
        entry_graph: Some("decode-unsigned-review".into()),
        ..Default::default()
    }
}

#[test]
fn random_words_remain_unsigned_in_both_runtimes_and_preserve_input_handles() {
    let p = program();
    let mut interpreter = Executor::new(&p, ExecutionLimits::default()).unwrap();
    let mut native = NativeContext::new(p.clone(), ExecutionLimits::default()).unwrap();
    let mut bits = 0x9e37_79b9u32;
    for _ in 0..512 {
        bits ^= bits << 13;
        bits ^= bits >> 17;
        bits ^= bits << 5;
        let bytes: Arc<[u8]> = bits.to_le_bytes().to_vec().into();
        let saved = bytes.clone();
        let input = Value::Bytes(bytes);
        let expected = Value::Integer(i128::from(bits));
        assert_eq!(
            interpreter
                .run_graph("decode-unsigned-review", vec![input.clone()])
                .unwrap(),
            vec![expected.clone()]
        );
        let handle = native.insert_value(input.clone()).unwrap();
        let output = native.primitive(0, 0, &[handle]);
        assert_eq!(native.value(output), Some(&expected));
        assert_eq!(native.value(handle), Some(&input));
        assert_eq!(saved.as_ref(), bits.to_le_bytes().as_slice());
    }
}

#[test]
fn only_four_bytes_are_accepted_and_sticky_failures_keep_the_input() {
    let p = program();
    let mut interpreter = Executor::new(&p, ExecutionLimits::default()).unwrap();
    for length in 0..=12 {
        let input = Value::Bytes(vec![255; length].into());
        let interpreted = interpreter.run_graph("decode-unsigned-review", vec![input.clone()]);
        let mut native = NativeContext::new(p.clone(), ExecutionLimits::default()).unwrap();
        let handle = native.insert_value(input.clone()).unwrap();
        let output = native.primitive(0, 0, &[handle]);
        if length == 4 {
            assert_eq!(interpreted.unwrap(), vec![Value::Integer(u32::MAX as i128)]);
            assert_eq!(
                native.value(output),
                Some(&Value::Integer(u32::MAX as i128))
            );
        } else {
            let bounds = RuntimeError::Bounds {
                graph: "decode-unsigned-review".into(),
                node: 42,
            };
            assert_eq!(interpreted, Err(bounds.clone()));
            assert_eq!(output, 0);
            assert_eq!(native.error, Some(bounds.clone()));
            assert_eq!(native.primitive(0, 0, &[handle]), 0);
            assert_eq!(native.error, Some(bounds));
        }
        assert_eq!(native.value(handle), Some(&input));
    }
}

#[test]
fn even_cached_small_results_charge_each_production() {
    let input = Value::Bytes(7u32.to_le_bytes().to_vec().into());
    let output = Value::Integer(7);
    let input_bytes = input.resident_bytes().unwrap();
    let output_bytes = output.resident_bytes().unwrap();
    let limits = ExecutionLimits {
        max_value_bytes: input_bytes + 2 * output_bytes,
        ..ExecutionLimits::default()
    };
    let mut native = NativeContext::new(program(), limits).unwrap();
    let handle = native.insert_value(input.clone()).unwrap();
    let first = native.primitive(0, 0, &[handle]);
    assert_ne!(first, 0);
    assert_eq!(native.primitive(0, 0, &[handle]), first);
    assert_eq!(native.primitive(0, 0, &[handle]), 0);
    assert_eq!(native.error, Some(RuntimeError::MemoryLimit));
    assert_eq!(native.value(handle), Some(&input));
    assert_eq!(native.value(first), Some(&output));
}
