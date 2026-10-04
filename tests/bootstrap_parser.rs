use g0::{bootstrap_compiler::compiler_document, execution::Executor, value::Value};

fn raw_compile(bytes: Vec<u8>) -> Result<Vec<Value>, g0::execution::RuntimeError> {
    let document = compiler_document();
    let contract = document.validated_contract().unwrap();
    Executor::new(&contract, g0::bootstrap_compiler::compiler_limits())
        .unwrap()
        .run_graph("compile", vec![Value::Bytes(bytes.into())])
}

fn valid_source() -> Vec<u8> {
    g0::program_binary::encode_program(&g0::program_binary::ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![g0::editor::GraphEditor::new().graph().clone()],
        schemas: vec![],
    })
    .unwrap()
}

#[test]
fn g0_parser_preserves_compilation_of_valid_graph_without_rust_decoding_source() {
    let source = valid_source();
    let output = raw_compile(source.clone()).unwrap();
    let [Value::Text(assembly)] = output.as_slice() else {
        panic!("assembly output")
    };
    assert!(assembly.starts_with(".text\n"), "{assembly}");
    assert_eq!(assembly.matches(".byte ").count(), source.len());
}

#[test]
fn g0_parser_can_parse_its_own_source() {
    let source = g0::program_binary::encode_program(&compiler_document()).unwrap();
    let output = raw_compile(source).unwrap();
    let [Value::Text(assembly)] = output.as_slice() else {
        panic!("assembly output")
    };
    assert!(assembly.starts_with(".text\n"), "{assembly}");
}

#[test]
fn g0_parser_rejects_bad_magic_without_rust_decoding_source() {
    let mut source = valid_source();
    source[0] = b'X';
    let result = raw_compile(source);
    assert_eq!(
        result.unwrap(),
        vec![Value::Text("G0 compiler: invalid container".into())]
    );
}

#[test]
fn g0_parser_rejects_trailing_bytes_without_rust_decoding_source() {
    let mut source = valid_source();
    source.push(0);
    let result = raw_compile(source);
    assert_eq!(
        result.unwrap(),
        vec![Value::Text("G0 compiler: invalid container".into())]
    );
}

#[test]
fn g0_parser_rejects_corrupt_graph_magic_without_rust_decoding_source() {
    let mut source = valid_source();
    let entry_length = u32::from_le_bytes(source[8..12].try_into().unwrap()) as usize;
    source[12 + entry_length + 8] = b'X';
    let result = raw_compile(source);
    assert!(
        matches!(
            result,
            Err(g0::execution::RuntimeError::TypeMismatch { .. })
        ) || result.unwrap() == vec![Value::Text("G0 compiler: invalid container".into())]
    );
}

#[test]
fn g0_parser_preserves_program_with_data_schema() {
    let source = g0::program_binary::encode_program(&g0::program_binary::ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![g0::editor::GraphEditor::new().graph().clone()],
        schemas: vec![g0::data_format::DataSchema {
            name: "record".into(),
            version: 1,
            fields: vec![g0::data_format::SchemaField {
                tag: 1,
                name: "enabled".into(),
                ty: g0::gir::SemanticType::Bool,
                requirement: g0::data_format::FieldRequirement::Required,
            }],
        }],
    })
    .unwrap();
    let output = raw_compile(source).unwrap();
    let [Value::Text(assembly)] = output.as_slice() else {
        panic!("assembly output")
    };
    assert!(assembly.starts_with(".text\n"), "{assembly}");
}

#[test]
fn g0_parser_rejects_unknown_capability_class_without_rust_decoding_source() {
    use g0::gir::*;
    let mut graph = g0::editor::GraphEditor::new().graph().clone();
    graph.nodes[0].operation = Operation::LocalExecute("artifact".into());
    graph.nodes[0].effects.insert(Effect::LocalExecution);
    graph.nodes[0].required_capabilities.insert(Capability::new(
        CapabilityClass::LocalExecution,
        "execute",
        "artifact",
        "scope",
    ));
    let mut source = g0::program_binary::encode_program(&g0::program_binary::ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![graph],
        schemas: vec![],
    })
    .unwrap();
    let document = compiler_document();
    let contract = document.validated_contract().unwrap();
    let ast = Executor::new(&contract, g0::bootstrap_compiler::compiler_limits())
        .unwrap()
        .run_graph(
            "reader-graph-ast",
            vec![Value::Bytes(source.clone().into()), Value::Integer(24)],
        )
        .unwrap();
    let [Value::Array(nodes)] = ast.as_slice() else {
        panic!("nodes")
    };
    let Value::Array(descriptor) = &nodes[0] else {
        panic!("descriptor")
    };
    let Value::Integer(capabilities) = descriptor[6] else {
        panic!("capability offset")
    };
    source[capabilities as usize + 4] = 11;
    let result = raw_compile(source);
    assert!(
        result.is_err()
            || result.unwrap() == vec![Value::Text("G0 compiler: invalid container".into())]
    );
}
