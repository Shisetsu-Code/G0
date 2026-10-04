use g0::{
    data_format::{DataSchema, FieldRequirement, SchemaField},
    gir::*,
    value::Value,
    value_codec::*,
};
use std::{collections::BTreeMap, sync::Arc};

fn registry() -> Vec<DataSchema> {
    vec![DataSchema {
        name: "Packet".into(),
        version: 1,
        fields: vec![
            SchemaField {
                tag: 1,
                name: "body".into(),
                ty: SemanticType::Text,
                requirement: FieldRequirement::Required,
            },
            SchemaField {
                tag: 4,
                name: "blob".into(),
                ty: SemanticType::Bytes,
                requirement: FieldRequirement::Optional,
            },
        ],
    }]
}

#[test]
fn canonical_values_round_trip_without_duplicate_models() {
    let int = SemanticType::Integer(IntegerType::new(-10, 10).unwrap());
    let samples = vec![
        (Value::Bool(true), SemanticType::Bool),
        (Value::Integer(-7), int.clone()),
        (Value::Text("ñ🙂".into()), SemanticType::Text),
        (Value::Bytes(vec![0, 255].into()), SemanticType::Bytes),
        (
            Value::Array(vec![Value::Integer(1), Value::Integer(2)].into()),
            SemanticType::Array(Box::new(int.clone()), 2),
        ),
        (
            Value::Option(None),
            SemanticType::Option(Box::new(int.clone())),
        ),
        (
            Value::Result(Err(Arc::new(Value::Text("err".into())))),
            SemanticType::Result(Box::new(int), Box::new(SemanticType::Text)),
        ),
        (
            Value::Record {
                schema: "Packet".into(),
                fields: Arc::new(BTreeMap::from([(
                    "body".into(),
                    Value::Text("one schema".into()),
                )])),
            },
            SemanticType::Record("Packet".into()),
        ),
        (
            Value::Variant {
                schema: "Packet".into(),
                tag: "blob".into(),
                payload: Arc::new(Value::Bytes(vec![42].into())),
            },
            SemanticType::Variant("Packet".into()),
        ),
    ];
    for (value, ty) in samples {
        let bytes = encode_value(&value, &ty, &registry(), CodecLimits::default()).unwrap();
        assert_eq!(
            decode_value(&bytes, &ty, &registry(), CodecLimits::default()).unwrap(),
            value
        );
        for end in 0..bytes.len() {
            assert!(decode_value(&bytes[..end], &ty, &registry(), CodecLimits::default()).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(decode_value(&extra, &ty, &registry(), CodecLimits::default()).is_err());
    }
}

#[test]
fn secrets_types_versions_and_budgets_are_enforced() {
    let secret = Value::Secret(Arc::new(Value::Text("hidden".into())));
    assert_eq!(
        encode_value(
            &secret,
            &SemanticType::Secret(Box::new(SemanticType::Text)),
            &[],
            Default::default()
        ),
        Err(CodecError::Sensitive)
    );
    let value = Value::Text("payload".into());
    let bytes = encode_value(&value, &SemanticType::Text, &[], Default::default()).unwrap();
    assert!(decode_value(&bytes, &SemanticType::Bytes, &[], Default::default()).is_err());
    assert_eq!(
        decode_value(
            &bytes,
            &SemanticType::Text,
            &[],
            CodecLimits {
                max_bytes: 4,
                ..Default::default()
            }
        ),
        Err(CodecError::Limit)
    );
    assert_eq!(
        encode_value(
            &value,
            &SemanticType::Text,
            &[],
            CodecLimits {
                max_bytes: 4,
                ..Default::default()
            }
        ),
        Err(CodecError::Limit)
    );
    let record = Value::Record {
        schema: "Packet".into(),
        fields: Arc::new(BTreeMap::from([("body".into(), value)])),
    };
    let ty = SemanticType::Record("Packet".into());
    let bytes = encode_value(&record, &ty, &registry(), Default::default()).unwrap();
    let mut changed = registry();
    changed[0].version = 2;
    assert_eq!(
        decode_value(&bytes, &ty, &changed, Default::default()),
        Err(CodecError::SchemaVersion)
    );
}
