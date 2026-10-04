use g0::{bootstrap_compiler::*, execution::Executor, gir::*, value::Value};

#[test]
fn native_type_depth_accepts_wide_trees_and_enforces_exact_depth_128() {
    fn wide(depth: u8) -> SemanticType {
        if depth == 0 {
            SemanticType::Bool
        } else {
            let child = wide(depth - 1);
            SemanticType::Result(Box::new(child.clone()), Box::new(child))
        }
    }
    let contract = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&contract, compiler_limits()).unwrap();
    let source = g0::graph_binary::encode_semantic_type(&wide(8)).unwrap();
    assert_eq!(source.len(), 511);
    assert_eq!(
        executor
            .run_graph(
                "validator-type-profile",
                vec![Value::Bytes(source.into()), Value::Integer(0)]
            )
            .unwrap(),
        vec![Value::Bool(true)]
    );
    for (depth, valid) in [(128, true), (129, false)] {
        let mut source = vec![14; depth];
        source.push(0);
        assert_eq!(
            executor
                .run_graph(
                    "validator-type-profile",
                    vec![Value::Bytes(source.into()), Value::Integer(0)]
                )
                .unwrap(),
            vec![Value::Bool(valid)]
        );
    }
}
#[test]
fn native_type_closure_checks_payloads_even_in_unused_nested_types() {
    let contract = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&contract, compiler_limits()).unwrap();
    let mut integer = vec![1];
    integer.extend(42i128.to_le_bytes());
    integer.extend(41i128.to_le_bytes());
    for mut source in [
        integer,
        vec![4, 0, 0, 0, 0, 0, 0, 0, 0],
        vec![6, 1, 0, 0, 0],
    ] {
        assert_eq!(
            executor
                .run_graph(
                    "validator-type-profile",
                    vec![Value::Bytes(source.clone().into()), Value::Integer(0)]
                )
                .unwrap(),
            vec![Value::Bool(false)]
        );
        source.insert(0, 14);
        assert_eq!(
            executor
                .run_graph(
                    "validator-type-profile",
                    vec![Value::Bytes(source.into()), Value::Integer(0)]
                )
                .unwrap(),
            vec![Value::Bool(false)]
        );
    }
}
#[test]
fn empty_schema_helper_reports_registry_presence() {
    let graph = g0::editor::GraphEditor::new().graph().clone();
    let compiler = compiler_document().validated_contract().unwrap();
    for with_schema in [false, true] {
        let document = g0::program_binary::ProgramDocument {
            entry_graph: "main".into(),
            graphs: vec![graph.clone()],
            schemas: if with_schema {
                vec![g0::data_format::DataSchema {
                    name: "unused".into(),
                    version: 1,
                    fields: vec![],
                }]
            } else {
                vec![]
            },
        };
        let source = g0::program_binary::encode_program(&document).unwrap();
        let output = Executor::new(&compiler, compiler_limits())
            .unwrap()
            .run_graph("validator-empty-schemas", vec![Value::Bytes(source.into())])
            .unwrap();
        assert_eq!(output, vec![Value::Bool(!with_schema)]);
    }
}
#[test]
fn native_type_shape_allows_named_records_but_rejects_reference_and_linear_values() {
    let scalar = SemanticType::Integer(IntegerType {
        min: i128::MIN,
        max: i128::MAX,
    });
    let contract = compiler_document().validated_contract().unwrap();
    for (ty, expected) in [
        (SemanticType::Array(Box::new(scalar.clone()), 4), true),
        (
            SemanticType::Result(Box::new(SemanticType::Bytes), Box::new(SemanticType::Bool)),
            true,
        ),
        (
            SemanticType::Slice(Box::new(SemanticType::Unique(Box::new(scalar)))),
            false,
        ),
        (SemanticType::Record("missing".into()), true),
        (SemanticType::Reference("missing".into()), false),
    ] {
        let bytes = g0::graph_binary::encode_semantic_type(&ty).unwrap();
        let output = Executor::new(&contract, compiler_limits())
            .unwrap()
            .run_graph(
                "validator-type-profile",
                vec![Value::Bytes(bytes.into()), Value::Integer(0)],
            )
            .unwrap();
        assert_eq!(output, vec![Value::Bool(expected)]);
    }
}
