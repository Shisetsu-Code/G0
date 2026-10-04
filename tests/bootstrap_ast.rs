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

#[test]
fn g0_reader_builds_program_graph_descriptors_in_file_order() {
    let mut a = g0::editor::GraphEditor::new().graph().clone();
    a.name = "alpha".into();
    let mut z = a.clone();
    z.name = "zeta".into();
    let source = encode_program(&ProgramDocument {
        entry_graph: "zeta".into(),
        graphs: vec![z, a],
        schemas: vec![],
    })
    .unwrap();
    let contract = compiler_document().validated_contract().unwrap();
    let output = Executor::new(&contract, compiler_limits())
        .unwrap()
        .run_graph(
            "reader-program-ast",
            vec![Value::Bytes(source.clone().into())],
        )
        .unwrap();
    let [Value::Array(graphs)] = output.as_slice() else {
        panic!("program AST")
    };
    assert_eq!(graphs.len(), 2);
    let mut framing = 20usize;
    for (descriptor, name) in graphs.iter().zip(["alpha", "zeta"]) {
        let Value::Array(fields) = descriptor else {
            panic!("graph descriptor")
        };
        assert_eq!(fields.len(), 7);
        let values: Vec<usize> = fields
            .iter()
            .map(|field| match field {
                Value::Integer(n) => *n as usize,
                _ => panic!("offset"),
            })
            .collect();
        let size = u32::from_le_bytes(source[framing..framing + 4].try_into().unwrap()) as usize;
        assert_eq!(values[0], framing + 4);
        assert_eq!(values[1], framing + 4 + size);
        assert_eq!(values[2], values[0] + 8);
        let length =
            u32::from_le_bytes(source[values[2]..values[2] + 4].try_into().unwrap()) as usize;
        assert_eq!(
            &source[values[2] + 4..values[2] + 4 + length],
            name.as_bytes()
        );
        for offset in &values[3..] {
            assert!(*offset < values[1]);
        }
        assert_eq!(
            u32::from_le_bytes(source[values[5]..values[5] + 4].try_into().unwrap()),
            1
        );
        assert_eq!(
            u32::from_le_bytes(source[values[6]..values[6] + 4].try_into().unwrap()),
            1
        );
        framing = values[1];
    }
}
