use g0::{
    data_format::{DataSchema, FieldRequirement, SchemaField},
    execution::{ExecutionLimits, Executor},
    gir::*,
    program::ProgramContract,
    value::Value,
};
use std::{collections::BTreeMap, sync::Arc};

fn int() -> SemanticType {
    SemanticType::Integer(IntegerType::new(0, 100).unwrap())
}
fn schema() -> DataSchema {
    DataSchema {
        name: "Message".into(),
        version: 1,
        fields: vec![
            SchemaField {
                tag: 1,
                name: "body".into(),
                ty: SemanticType::Text,
                requirement: FieldRequirement::Required,
            },
            SchemaField {
                tag: 2,
                name: "count".into(),
                ty: int(),
                requirement: FieldRequirement::Optional,
            },
        ],
    }
}
fn program(
    operation: Operation,
    input_types: Vec<SemanticType>,
    output: SemanticType,
) -> ProgramContract {
    let ports: Vec<_> = input_types
        .into_iter()
        .enumerate()
        .map(|(id, ty)| Port {
            id: id as u16,
            name: format!("in{id}"),
            ty,
        })
        .collect();
    let mut graph = Graph::new("op");
    graph.inputs = ports.clone();
    graph.outputs = vec![Port {
        id: 0,
        name: "out".into(),
        ty: output,
    }];
    graph.nodes.push(Node {
        id: 1,
        operation,
        inputs: ports,
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
        schemas: vec![schema()],
        ..Default::default()
    }
}
fn execute(
    operation: Operation,
    types: Vec<SemanticType>,
    output: SemanticType,
    inputs: Vec<Value>,
) -> Value {
    let program = program(operation, types, output);
    let bytes = g0::graph_binary::encode_graph(&program.graphs[0]).unwrap();
    let decoded = g0::graph_binary_decode::decode_graph(&bytes).unwrap();
    assert_eq!(g0::graph_binary::encode_graph(&decoded).unwrap(), bytes);
    Executor::new(&program, ExecutionLimits::default())
        .unwrap()
        .run_graph("op", inputs)
        .unwrap()
        .remove(0)
}

#[test]
fn arrays_are_constructed_and_access_is_optional() {
    let ty = SemanticType::Array(Box::new(int()), 2);
    let array = execute(
        Operation::MakeArray,
        vec![int(), int()],
        ty.clone(),
        vec![Value::Integer(3), Value::Integer(7)],
    );
    assert_eq!(
        array,
        Value::Array(vec![Value::Integer(3), Value::Integer(7)].into())
    );
    for (index, expected) in [(0, Some(3)), (1, Some(7)), (2, None)] {
        assert_eq!(
            execute(
                Operation::Index,
                vec![ty.clone(), int()],
                SemanticType::Option(Box::new(int())),
                vec![array.clone(), Value::Integer(index)]
            ),
            Value::Option(expected.map(|v| Arc::new(Value::Integer(v))))
        );
    }
}

#[test]
fn records_preserve_schema_and_optional_fields() {
    let record = execute(
        Operation::MakeRecord {
            schema: "Message".into(),
            fields: vec!["body".into()],
        },
        vec![SemanticType::Text],
        SemanticType::Record("Message".into()),
        vec![Value::Text("hola".into())],
    );
    assert_eq!(
        record,
        Value::Record {
            schema: "Message".into(),
            fields: Arc::new(BTreeMap::from([(
                "body".into(),
                Value::Text("hola".into())
            )]))
        }
    );
    assert_eq!(
        execute(
            Operation::Field {
                name: "body".into()
            },
            vec![SemanticType::Record("Message".into())],
            SemanticType::Text,
            vec![record.clone()]
        ),
        Value::Text("hola".into())
    );
    assert_eq!(
        execute(
            Operation::Field {
                name: "count".into()
            },
            vec![SemanticType::Record("Message".into())],
            SemanticType::Option(Box::new(int())),
            vec![record]
        ),
        Value::Option(None)
    );
    let invalid = program(
        Operation::MakeRecord {
            schema: "Message".into(),
            fields: vec!["count".into()],
        },
        vec![int()],
        SemanticType::Record("Message".into()),
    );
    assert!(Executor::new(&invalid, Default::default()).is_err());
}

#[test]
fn text_bytes_and_explicit_decode_keep_errors_as_values() {
    assert_eq!(
        execute(
            Operation::TextConcat,
            vec![SemanticType::Text, SemanticType::Text],
            SemanticType::Text,
            vec![Value::Text("ñ".into()), Value::Text("🙂".into())]
        ),
        Value::Text("ñ🙂".into())
    );
    let result = SemanticType::Result(Box::new(SemanticType::Text), Box::new(SemanticType::Bytes));
    for bytes in [vec![0xff], "ñ🙂".as_bytes().to_vec()] {
        let decoded = execute(
            Operation::DecodeUtf8,
            vec![SemanticType::Bytes],
            result.clone(),
            vec![Value::Bytes(bytes.clone().into())],
        );
        let expected = match String::from_utf8(bytes.clone()) {
            Ok(text) => Value::Result(Ok(Arc::new(Value::Text(text.into())))),
            Err(_) => Value::Result(Err(Arc::new(Value::Bytes(bytes.into())))),
        };
        assert_eq!(decoded, expected);
    }
}

#[test]
fn variants_check_tag_and_payload_type() {
    let ty = SemanticType::Variant("Message".into());
    let variant = execute(
        Operation::MakeVariant {
            schema: "Message".into(),
            tag: "body".into(),
        },
        vec![SemanticType::Text],
        ty.clone(),
        vec![Value::Text("payload".into())],
    );
    assert_eq!(
        execute(
            Operation::VariantPayload {
                tag: "count".into()
            },
            vec![ty],
            SemanticType::Option(Box::new(int())),
            vec![variant]
        ),
        Value::Option(None)
    );
    assert!(
        Executor::new(
            &program(
                Operation::MakeVariant {
                    schema: "Message".into(),
                    tag: "absent".into()
                },
                vec![int()],
                SemanticType::Variant("Message".into())
            ),
            Default::default()
        )
        .is_err()
    );
}

#[test]
fn wrappers_lengths_and_integer_formatting_are_executable() {
    assert_eq!(
        execute(
            Operation::FormatInteger,
            vec![int()],
            SemanticType::Text,
            vec![Value::Integer(42)]
        ),
        Value::Text("42".into())
    );
    assert_eq!(
        execute(
            Operation::EncodeUtf8,
            vec![SemanticType::Text],
            SemanticType::Bytes,
            vec![Value::Text("ñ".into())]
        ),
        Value::Bytes(vec![0xc3, 0xb1].into())
    );
    assert_eq!(
        execute(
            Operation::BytesConcat,
            vec![SemanticType::Bytes, SemanticType::Bytes],
            SemanticType::Bytes,
            vec![
                Value::Bytes(vec![0, 255].into()),
                Value::Bytes(vec![42].into())
            ]
        ),
        Value::Bytes(vec![0, 255, 42].into())
    );
    let length = SemanticType::Integer(IntegerType::new(0, u64::MAX as i128).unwrap());
    assert_eq!(
        execute(
            Operation::Length,
            vec![SemanticType::Bytes],
            length,
            vec![Value::Bytes(vec![1, 2, 3].into())]
        ),
        Value::Integer(3)
    );
    let option = SemanticType::Option(Box::new(int()));
    let some = execute(
        Operation::Some,
        vec![int()],
        option.clone(),
        vec![Value::Integer(7)],
    );
    assert_eq!(
        execute(
            Operation::UnwrapOr,
            vec![option.clone(), int()],
            int(),
            vec![some, Value::Integer(5)]
        ),
        Value::Integer(7)
    );
    let none = execute(Operation::None, vec![], option.clone(), vec![]);
    assert_eq!(
        execute(
            Operation::UnwrapOr,
            vec![option, int()],
            int(),
            vec![none, Value::Integer(5)]
        ),
        Value::Integer(5)
    );
    let result = SemanticType::Result(Box::new(int()), Box::new(SemanticType::Text));
    assert_eq!(
        execute(
            Operation::Ok,
            vec![int()],
            result.clone(),
            vec![Value::Integer(7)]
        ),
        Value::Result(Ok(Arc::new(Value::Integer(7))))
    );
    assert_eq!(
        execute(
            Operation::Err,
            vec![SemanticType::Text],
            result,
            vec![Value::Text("explicit".into())]
        ),
        Value::Result(Err(Arc::new(Value::Text("explicit".into()))))
    );
}
