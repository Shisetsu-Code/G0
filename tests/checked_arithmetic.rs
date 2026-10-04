use g0::{execution::Executor, gir::*, program::ProgramContract, value::Value};
use std::sync::Arc;

fn int() -> SemanticType {
    SemanticType::Integer(IntegerType {
        min: i128::MIN,
        max: i128::MAX,
    })
}
fn checked() -> SemanticType {
    SemanticType::Result(Box::new(int()), Box::new(SemanticType::Bool))
}
fn program(
    operation: Operation,
    inputs: Vec<SemanticType>,
    output: SemanticType,
) -> ProgramContract {
    let mut graph = Graph::new("checked");
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
    graph.nodes = vec![Node {
        id: 1,
        operation,
        inputs: graph.inputs.clone(),
        outputs: graph.outputs.clone(),
        effects: Default::default(),
        required_capabilities: Default::default(),
    }];
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
        .chain([Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        }])
        .collect();
    ProgramContract {
        graphs: vec![graph],
        ..Default::default()
    }
}
#[test]
fn overflow_is_a_typed_result_and_native_primitives_agree() {
    for (op, a, b, expected) in [
        (Operation::CheckedAdd, i128::MAX, 1, None),
        (Operation::CheckedAdd, i128::MIN, 1, Some(i128::MIN + 1)),
        (Operation::CheckedSub, i128::MIN, 1, None),
        (Operation::CheckedSub, 0, i128::MIN, None),
        (Operation::CheckedMul, i128::MIN, -1, None),
        (Operation::CheckedMul, i128::MIN, 1, Some(i128::MIN)),
        (Operation::CheckedMul, 0, i128::MAX, Some(0)),
    ] {
        let p = program(op, vec![int(), int()], checked());
        let expected = Value::Result(match expected {
            Some(n) => Ok(Arc::new(Value::Integer(n))),
            None => Err(Arc::new(Value::Bool(true))),
        });
        let args = vec![Value::Integer(a), Value::Integer(b)];
        assert_eq!(
            Executor::new(&p, Default::default())
                .unwrap()
                .run_graph("checked", args.clone())
                .unwrap(),
            vec![expected.clone()]
        );
        let mut context =
            g0::native_aggregate_runtime::NativeContext::new(p.clone(), Default::default())
                .unwrap();
        let handles = args
            .into_iter()
            .map(|a| context.insert_value(a).unwrap())
            .collect::<Vec<_>>();
        let handle = context.primitive(0, 0, &handles);
        assert_eq!(context.value(handle), Some(&expected));
        let bytes = g0::graph_binary::encode_graph(&p.graphs[0]).unwrap();
        assert_eq!(
            g0::graph_binary_decode::decode_graph(&bytes).unwrap(),
            p.graphs[0]
        );
        let mut legacy = bytes;
        legacy[6..8].copy_from_slice(&8u16.to_le_bytes());
        assert!(g0::graph_binary_decode::decode_graph(&legacy).is_err());
    }
}
#[test]
fn result_is_ok_observes_the_tag_without_unwrapping() {
    let p = program(Operation::ResultIsOk, vec![checked()], SemanticType::Bool);
    for (value, expected) in [
        (Value::Result(Ok(Arc::new(Value::Integer(i128::MIN)))), true),
        (Value::Result(Err(Arc::new(Value::Bool(false)))), false),
    ] {
        assert_eq!(
            Executor::new(&p, Default::default())
                .unwrap()
                .run_graph("checked", vec![value.clone()])
                .unwrap(),
            vec![Value::Bool(expected)]
        );
        let mut context =
            g0::native_aggregate_runtime::NativeContext::new(p.clone(), Default::default())
                .unwrap();
        let input = context.insert_value(value).unwrap();
        let output = context.primitive(0, 0, &[input]);
        assert_eq!(context.value(output), Some(&Value::Bool(expected)));
    }
    let mut bytes = g0::graph_binary::encode_graph(&p.graphs[0]).unwrap();
    assert_eq!(
        g0::graph_binary_decode::decode_graph(&bytes).unwrap(),
        p.graphs[0]
    );
    bytes[6..8].copy_from_slice(&8u16.to_le_bytes());
    assert!(g0::graph_binary_decode::decode_graph(&bytes).is_err());
}
#[test]
fn checked_arithmetic_rejects_narrow_success_types() {
    let narrow = SemanticType::Result(
        Box::new(SemanticType::Integer(IntegerType { min: 0, max: 10 })),
        Box::new(SemanticType::Bool),
    );
    let p = program(Operation::CheckedAdd, vec![int(), int()], narrow);
    assert!(Executor::new(&p, Default::default()).is_err());
}
