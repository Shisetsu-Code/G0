use g0::{bootstrap_compiler::*, execution::Executor, gir::*, program_binary::*, value::Value};

#[test]
fn g0_reader_builds_node_offsets_from_binary_not_host_ast() {
    let document = compiler_document();
    let contract = document.validated_contract().unwrap();
    let source = encode_program(&ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![g0::editor::GraphEditor::new().graph().clone()],
        schemas: vec![],
    })
    .unwrap();
    let start = 12 + 4 + 8; // framing: header, four-byte entry name, graph count, blob length
    let mut executor = Executor::new(&contract, compiler_limits()).unwrap();
    let output = executor
        .run_graph(
            "reader-graph-ast",
            vec![Value::Bytes(source.into()), Value::Integer(start)],
        )
        .unwrap();
    let [Value::Array(nodes)] = output.as_slice() else {
        panic!("AST nodes")
    };
    assert_eq!(nodes.len(), 1);
    let Value::Array(fields) = &nodes[0] else {
        panic!("AST descriptor")
    };
    assert_eq!(fields.len(), 8);
    assert_eq!(fields[0], Value::Integer(1));
    assert!(
        matches!((&fields[1],&fields[7]),(Value::Integer(start),Value::Integer(end)) if end>start)
    );
    let _ = Operation::Const(Literal::Bool(true));
}

#[test]
fn g0_reader_builds_edge_endpoints_from_binary_not_host_ast() {
    let document = compiler_document();
    let contract = document.validated_contract().unwrap();
    let source = encode_program(&ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![g0::editor::GraphEditor::new().graph().clone()],
        schemas: vec![],
    })
    .unwrap();
    let output = Executor::new(&contract, compiler_limits())
        .unwrap()
        .run_graph(
            "reader-graph-edges",
            vec![Value::Bytes(source.into()), Value::Integer(24)],
        )
        .unwrap();
    let [Value::Array(edges)] = output.as_slice() else {
        panic!("edge AST")
    };
    assert_eq!(edges.len(), 1);
    assert_eq!(
        edges[0],
        Value::Array(
            vec![
                Value::Integer(1),
                Value::Integer(1),
                Value::Integer(0),
                Value::Integer(1),
                Value::Integer(0),
                Value::Integer(0)
            ]
            .into()
        )
    );
}
