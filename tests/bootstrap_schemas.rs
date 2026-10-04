use g0::{
    bootstrap_compiler::*, data_format::*, execution::Executor, gir::*, program_binary::*,
    value::Value,
};
fn source() -> Vec<u8> {
    encode_program(&ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![g0::editor::GraphEditor::new().graph().clone()],
        schemas: vec![DataSchema {
            name: "Sample".into(),
            version: 1,
            fields: vec![
                SchemaField {
                    tag: 2,
                    name: "value".into(),
                    ty: SemanticType::Integer(IntegerType {
                        min: -100,
                        max: 100,
                    }),
                    requirement: FieldRequirement::Required,
                },
                SchemaField {
                    tag: 7,
                    name: "flag".into(),
                    ty: SemanticType::Bool,
                    requirement: FieldRequirement::Optional,
                },
            ],
        }],
    })
    .unwrap()
}
#[test]
fn g0_registry_rejects_zero_version_duplicate_names_and_field_tags() {
    let source = source();
    let contract = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&contract, compiler_limits()).unwrap();
    let valid = executor
        .run_graph(
            "validator-program-schemas",
            vec![Value::Bytes(source.clone().into())],
        )
        .unwrap();
    assert_eq!(valid, vec![Value::Bool(true)]);
    let marker = source.windows(6).position(|w| w == b"Sample").unwrap();
    let version = marker + 6;
    let fields = version + 8;
    let field_name = fields + 8;
    let second_tag = field_name + 5 + 4 + 33 + 1;
    for at in [version, fields, second_tag] {
        let mut invalid = source.clone();
        invalid[at..at + 4].copy_from_slice(&0u32.to_le_bytes());
        assert_eq!(
            executor
                .run_graph(
                    "validator-program-schemas",
                    vec![Value::Bytes(invalid.into())]
                )
                .unwrap(),
            vec![Value::Bool(false)],
            "offset {at}"
        );
    }
}
#[test]
fn g0_schema_types_resolve_nested_names_and_reject_hidden_linear_values() {
    let mut document = decode_program(&source()).unwrap();
    document.schemas[0].fields[1].ty =
        SemanticType::Option(Box::new(SemanticType::Record("Sample".into())));
    let source = encode_program(&document).unwrap();
    let contract = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&contract, compiler_limits()).unwrap();
    assert_eq!(
        executor
            .run_graph(
                "validator-program-schema-types",
                vec![Value::Bytes(source.clone().into())]
            )
            .unwrap(),
        vec![Value::Bool(true)]
    );
    let mut unknown = source.clone();
    let at = unknown.windows(6).rposition(|b| b == b"Sample").unwrap();
    unknown[at..at + 6].copy_from_slice(b"Absent");
    assert_eq!(
        executor
            .run_graph(
                "validator-program-schema-types",
                vec![Value::Bytes(unknown.into())]
            )
            .unwrap(),
        vec![Value::Bool(false)]
    );
    document.schemas[0].fields[1].ty =
        SemanticType::Option(Box::new(SemanticType::Unique(Box::new(SemanticType::Bool))));
    let source = encode_program(&document).unwrap();
    assert_eq!(
        executor
            .run_graph(
                "validator-program-schema-types",
                vec![Value::Bytes(source.into())]
            )
            .unwrap(),
        vec![Value::Bool(false)]
    );
}
#[test]
fn g0_graph_ports_resolve_named_types_against_the_program_registry() {
    let mut document = decode_program(&source()).unwrap();
    document.graphs[0].inputs.push(Port {
        id: 0,
        name: "item".into(),
        ty: SemanticType::Record("Sample".into()),
    });
    let source = encode_program(&document).unwrap();
    let contract = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&contract, compiler_limits()).unwrap();
    assert_eq!(
        executor
            .run_graph(
                "validator-program-port-types",
                vec![Value::Bytes(source.clone().into())]
            )
            .unwrap(),
        vec![Value::Bool(true)]
    );
    let mut invalid = source;
    let at = invalid.windows(6).position(|b| b == b"Sample").unwrap();
    invalid[at..at + 6].copy_from_slice(b"Absent");
    assert_eq!(
        executor
            .run_graph(
                "validator-program-port-types",
                vec![Value::Bytes(invalid.into())]
            )
            .unwrap(),
        vec![Value::Bool(false)]
    );
}

#[test]
fn g0_direct_compiler_accepts_verified_registry_and_rejects_raw_invalid_version() {
    let source = source();
    let contract = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&contract, compiler_limits()).unwrap();
    let result = executor
        .run_graph("compile-direct", vec![Value::Bytes(source.clone().into())])
        .unwrap();
    let [Value::Text(assembly)] = result.as_slice() else {
        panic!("assembly")
    };
    assert!(assembly.starts_with(".text"));
    let mut invalid = source;
    let at = invalid.windows(6).position(|b| b == b"Sample").unwrap() + 6;
    invalid[at..at + 4].copy_from_slice(&0u32.to_le_bytes());
    let result = executor
        .run_graph("compile-direct", vec![Value::Bytes(invalid.into())])
        .unwrap();
    let [Value::Text(diagnostic)] = result.as_slice() else {
        panic!("diagnostic")
    };
    assert!(diagnostic.starts_with("G0 compiler:"));
}
#[test]
fn g0_schema_and_field_tables_preserve_names_types_tags_and_requirements() {
    let source = source();
    let contract = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&contract, compiler_limits()).unwrap();
    let schemas = executor
        .run_graph(
            "reader-program-schemas",
            vec![Value::Bytes(source.clone().into())],
        )
        .unwrap();
    let [Value::Array(schemas)] = schemas.as_slice() else {
        panic!("schemas")
    };
    assert_eq!(schemas.len(), 1);
    let Value::Array(schema) = &schemas[0] else {
        panic!("schema")
    };
    let Value::Integer(name_at) = schema[2] else {
        panic!("name")
    };
    assert_eq!(
        &source[name_at as usize + 4..name_at as usize + 10],
        b"Sample"
    );
    let Value::Integer(fields_at) = schema[4] else {
        panic!("fields")
    };
    let fields = executor
        .run_graph(
            "reader-schema-fields",
            vec![
                Value::Bytes(source.clone().into()),
                Value::Integer(fields_at),
            ],
        )
        .unwrap();
    let [Value::Array(fields)] = fields.as_slice() else {
        panic!("fields")
    };
    assert_eq!(fields.len(), 2);
    for (field, tag, type_tag, required) in [(&fields[0], 2, 1, 0), (&fields[1], 7, 0, 1)] {
        let Value::Array(field) = field else {
            panic!("field")
        };
        assert_eq!(field[0], Value::Integer(tag));
        let Value::Integer(type_at) = field[3] else {
            panic!("type")
        };
        let Value::Integer(requirement_at) = field[4] else {
            panic!("requirement")
        };
        assert_eq!(source[type_at as usize], type_tag);
        assert_eq!(source[requirement_at as usize], required);
    }
}
