use g0::{
    execution::{ExecutionLimits, Executor, RuntimeError},
    gir::*,
    program::ProgramContract,
    value::Value,
};

fn int() -> SemanticType {
    SemanticType::Integer(IntegerType::new(0, i128::MAX).unwrap())
}
fn program(op: Operation, inputs: Vec<SemanticType>, output: SemanticType) -> ProgramContract {
    let mut graph = Graph::new("collection");
    graph.inputs = inputs
        .into_iter()
        .enumerate()
        .map(|(id, ty)| Port {
            id: id as u16,
            name: format!("in{id}"),
            ty,
        })
        .collect();
    graph.outputs = vec![Port {
        id: 0,
        name: "out".into(),
        ty: output,
    }];
    graph.nodes.push(Node {
        id: 1,
        operation: op,
        inputs: graph.inputs.clone(),
        outputs: graph.outputs.clone(),
        effects: Default::default(),
        required_capabilities: Default::default(),
    });
    graph.edges = graph
        .inputs
        .iter()
        .map(|p| Edge {
            from: SourceEndpoint::GraphInput(p.id),
            to: TargetEndpoint::NodeInput {
                node: 1,
                port: p.id,
            },
        })
        .collect();
    graph.edges.push(Edge {
        from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
        to: TargetEndpoint::GraphOutput(0),
    });
    ProgramContract {
        graphs: vec![graph],
        ..Default::default()
    }
}
fn run(
    op: Operation,
    types: Vec<SemanticType>,
    out: SemanticType,
    values: Vec<Value>,
) -> Result<Vec<Value>, RuntimeError> {
    let p = program(op, types, out);
    let bytes = g0::graph_binary::encode_graph(&p.graphs[0]).unwrap();
    assert_eq!(
        g0::graph_binary_decode::decode_graph(&bytes).unwrap(),
        p.graphs[0]
    );
    let mut legacy = bytes.clone();
    legacy[6..8].copy_from_slice(&7u16.to_le_bytes());
    assert!(g0::graph_binary_decode::decode_graph(&legacy).is_err());
    Executor::new(&p, Default::default())?.run_graph("collection", values)
}
#[test]
fn slices_check_bounds_without_wrapping_and_allow_empty_end() {
    for (start, len, expected) in [
        (1, 2, Some(vec![2, 3])),
        (4, 0, Some(vec![])),
        (4, 1, None),
        (i128::MAX, i128::MAX, None),
    ] {
        let result = run(
            Operation::BytesSlice,
            vec![SemanticType::Bytes, int(), int()],
            SemanticType::Bytes,
            vec![
                Value::Bytes(vec![1, 2, 3, 4].into()),
                Value::Integer(start),
                Value::Integer(len),
            ],
        );
        if let Some(bytes) = expected {
            assert_eq!(result.unwrap(), [Value::Bytes(bytes.into())]);
        } else {
            assert!(matches!(result, Err(RuntimeError::Bounds { .. })));
        }
    }
}
#[test]
fn array_concat_preserves_order_and_builds_dynamic_slices() {
    let out = SemanticType::Slice(Box::new(int()));
    assert_eq!(
        run(
            Operation::ArrayConcat,
            vec![SemanticType::Array(Box::new(int()), 2), out.clone()],
            out,
            vec![
                Value::Array(vec![Value::Integer(1), Value::Integer(2)].into()),
                Value::Array(vec![Value::Integer(3)].into())
            ]
        )
        .unwrap(),
        [Value::Array(
            vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)].into()
        )]
    );
}
#[test]
fn range_and_byte_construction_have_explicit_types_and_limits() {
    assert_eq!(
        run(
            Operation::Range,
            vec![int()],
            SemanticType::Slice(Box::new(int())),
            vec![Value::Integer(3)]
        )
        .unwrap(),
        [Value::Array(
            vec![Value::Integer(0), Value::Integer(1), Value::Integer(2)].into()
        )]
    );
    let byte = SemanticType::Integer(IntegerType::new(0, 255).unwrap());
    assert_eq!(
        run(
            Operation::BytesFromArray,
            vec![SemanticType::Slice(Box::new(byte))],
            SemanticType::Bytes,
            vec![Value::Array(
                vec![Value::Integer(0), Value::Integer(255)].into()
            )]
        )
        .unwrap(),
        [Value::Bytes(vec![0, 255].into())]
    );
    let bad = program(
        Operation::BytesFromArray,
        vec![SemanticType::Slice(Box::new(int()))],
        SemanticType::Bytes,
    );
    assert!(Executor::new(&bad, Default::default()).is_err());
    let p = program(
        Operation::Range,
        vec![int()],
        SemanticType::Slice(Box::new(int())),
    );
    let mut executor = Executor::new(
        &p,
        ExecutionLimits {
            max_value_bytes: 10_000,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        executor.run_graph("collection", vec![Value::Integer(i128::MAX)]),
        Err(RuntimeError::MemoryLimit)
    );
    assert_eq!(
        run(
            Operation::Range,
            vec![int()],
            SemanticType::Slice(Box::new(int())),
            vec![Value::Integer(0)]
        )
        .unwrap(),
        [Value::Array(vec![].into())]
    );
}

#[test]
fn result_unwrap_uses_success_payload_or_explicit_fallback() {
    use std::sync::Arc;
    let p = program(
        Operation::UnwrapOr,
        vec![
            SemanticType::Result(Box::new(SemanticType::Text), Box::new(SemanticType::Bytes)),
            SemanticType::Text,
        ],
        SemanticType::Text,
    );
    for (input, expected) in [
        (
            Value::Result(Ok(Arc::new(Value::Text("valid".into())))),
            "valid",
        ),
        (
            Value::Result(Err(Arc::new(Value::Bytes(vec![255].into())))),
            "fallback",
        ),
    ] {
        let mut executor = Executor::new(&p, Default::default()).unwrap();
        assert_eq!(
            executor
                .run_graph("collection", vec![input, Value::Text("fallback".into())])
                .unwrap(),
            [Value::Text(expected.into())]
        );
    }
}
