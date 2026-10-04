use g0::{bootstrap_compiler::*, execution::Executor, value::Value};

#[test]
fn g0_graph_reader_rejects_a_node_cycle_before_native_emission() {
    use g0::gir::*;
    let ty = SemanticType::Integer(IntegerType { min: 42, max: 42 });
    let p = || Port {
        id: 0,
        name: "value".into(),
        ty: ty.clone(),
    };
    let node = |id, operation, inputs| Node {
        id,
        operation,
        inputs,
        outputs: vec![p()],
        effects: Default::default(),
        required_capabilities: Default::default(),
    };
    let mut graph = Graph::new("main");
    graph.outputs = vec![p()];
    graph.nodes = vec![
        node(1, Operation::Const(Literal::Integer(42)), vec![]),
        node(2, Operation::ConvertChecked, vec![p()]),
    ];
    graph.edges = vec![
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::NodeInput { node: 2, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    let mut bytes = g0::graph_binary::encode_graph(&graph).unwrap();
    let at = bytes.len() - 24 + 1;
    assert_eq!(&bytes[at..at + 4], &1u32.to_le_bytes());
    bytes[at..at + 4].copy_from_slice(&2u32.to_le_bytes());
    let len = bytes.len() as i128;
    let contract = compiler_document().validated_contract().unwrap();
    let output = Executor::new(&contract, compiler_limits())
        .unwrap()
        .run_graph(
            "reader-graph",
            vec![Value::Bytes(bytes.into()), Value::Integer(0)],
        );
    assert!(!matches!(output,Ok(values) if values==vec![Value::Integer(len)]));
}

#[test]
fn g0_identifier_reader_skips_membership_scans_above_the_seen_maximum() {
    let mut bytes = 100u32.to_le_bytes().to_vec();
    for id in 0u16..100 {
        bytes.extend(id.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.push(b'x');
        bytes.push(0);
    }
    let contract = compiler_document().validated_contract().unwrap();
    let mut limits = compiler_limits();
    limits.max_steps = 25_000;
    let len = bytes.len() as i128;
    let output = Executor::new(&contract, limits)
        .unwrap()
        .run_graph(
            "reader-list-reader-port",
            vec![Value::Bytes(bytes.into()), Value::Integer(0)],
        )
        .unwrap();
    assert_eq!(output, vec![Value::Integer(len)]);
}

#[test]
fn g0_integer_assignment_uses_signed_byte_comparison_under_small_step_budget() {
    use g0::gir::*;
    let narrow = SemanticType::Integer(IntegerType { min: -7, max: 42 });
    let wide = SemanticType::Integer(IntegerType {
        min: i128::MIN,
        max: i128::MAX,
    });
    let mut bytes = g0::graph_binary::encode_semantic_type(&narrow).unwrap();
    let target = bytes.len() as i128;
    bytes.extend(g0::graph_binary::encode_semantic_type(&wide).unwrap());
    let contract = compiler_document().validated_contract().unwrap();
    let mut limits = compiler_limits();
    limits.max_steps = 1000;
    let output = Executor::new(&contract, limits)
        .unwrap()
        .run_graph(
            "validator-type-assignable",
            vec![
                Value::Bytes(bytes.into()),
                Value::Integer(0),
                Value::Integer(target),
            ],
        )
        .unwrap();
    assert_eq!(output, vec![Value::Bool(true)]);
}

#[test]
fn g0_graph_reader_rejects_literal_outside_declared_output_interval() {
    let graph = g0::editor::GraphEditor::new().graph().clone();
    let mut source = g0::graph_binary::encode_graph(&graph).unwrap();
    let contract = compiler_document().validated_contract().unwrap();
    let nodes = Executor::new(&contract, compiler_limits())
        .unwrap()
        .run_graph(
            "reader-graph-ast",
            vec![Value::Bytes(source.clone().into()), Value::Integer(0)],
        )
        .unwrap();
    let [Value::Array(rows)] = nodes.as_slice() else {
        panic!("AST")
    };
    let Value::Array(row) = &rows[0] else {
        panic!("node")
    };
    let Value::Integer(at) = row[2] else {
        panic!("opcode")
    };
    assert_eq!(source[at as usize], 0);
    assert_eq!(source[at as usize + 1], 1);
    source[at as usize + 2..at as usize + 18].copy_from_slice(&43i128.to_le_bytes());
    let len = source.len() as i128;
    let output = Executor::new(&contract, compiler_limits())
        .unwrap()
        .run_graph(
            "reader-graph",
            vec![Value::Bytes(source.into()), Value::Integer(0)],
        );
    assert!(!matches!(output,Ok(ref values) if values==&vec![Value::Integer(len)]));
}

fn read_type(bytes: Vec<u8>) -> i128 {
    let contract = compiler_document().validated_contract().unwrap();
    let output = Executor::new(&contract, compiler_limits())
        .unwrap()
        .run_graph(
            "reader-type",
            vec![Value::Bytes(bytes.into()), Value::Integer(0)],
        )
        .unwrap();
    let [Value::Integer(end)] = output.as_slice() else {
        panic!("type cursor")
    };
    *end
}

#[test]
fn g0_type_reader_validates_full_signed_integer_bounds() {
    for (min, max, valid) in [
        (i128::MIN, i128::MAX, true),
        (-256, -255, true),
        (-1, 0, true),
        (0, -1, false),
        (256, 255, false),
        (i128::MAX, i128::MIN, false),
        (42, 42, true),
    ] {
        let mut source = vec![1];
        source.extend_from_slice(&min.to_le_bytes());
        source.extend_from_slice(&max.to_le_bytes());
        assert_eq!(read_type(source) == 33, valid, "{min}..{max}");
    }
}

#[test]
fn g0_signed_decoder_preserves_i128_boundary_values() {
    let contract = compiler_document().validated_contract().unwrap();
    for value in [
        i128::MIN,
        i128::MIN + 1,
        -257,
        -256,
        -255,
        -1,
        0,
        1,
        255,
        256,
        i128::MAX - 1,
        i128::MAX,
    ] {
        let output = Executor::new(&contract, compiler_limits())
            .unwrap()
            .run_graph(
                "reader-i128",
                vec![
                    Value::Bytes(value.to_le_bytes().to_vec().into()),
                    Value::Integer(0),
                ],
            )
            .unwrap();
        assert_eq!(output, vec![Value::Integer(value)]);
    }
}

#[test]
fn g0_type_reader_validates_decimal_and_bigfloat_precision() {
    for (tag, precision, valid) in [
        (4, 0u32, false),
        (4, 1, true),
        (6, 0, false),
        (6, 1, false),
        (6, 2, true),
    ] {
        let mut source = vec![tag];
        source.extend_from_slice(&precision.to_le_bytes());
        if tag == 4 {
            source.extend_from_slice(&(-17i32).to_le_bytes());
        }
        let length = source.len() as i128;
        assert_eq!(
            read_type(source) == length,
            valid,
            "tag {tag} precision {precision}"
        );
    }
}

#[test]
fn g0_graph_reader_rejects_operation_newer_than_graph_version() {
    let graph = compiler_document()
        .graphs
        .into_iter()
        .find(|g| g.name == "reader-i128-body")
        .unwrap();
    let mut source = g0::graph_binary::encode_graph(&graph).unwrap();
    source[6..8].copy_from_slice(&8u16.to_le_bytes());
    let contract = compiler_document().validated_contract().unwrap();
    let output = Executor::new(&contract, compiler_limits())
        .unwrap()
        .run_graph(
            "reader-graph",
            vec![Value::Bytes(source.clone().into()), Value::Integer(0)],
        );
    if let Ok(output) = output {
        assert_ne!(output, vec![Value::Integer(source.len() as i128)]);
    }
}

#[test]
fn g0_raw_parser_checks_integer_codec_minor_version() {
    let graph = compiler_document()
        .graphs
        .into_iter()
        .find(|graph| graph.name == "reader-i128-word")
        .unwrap();
    let source = g0::graph_binary::encode_graph(&graph).unwrap();
    let contract = compiler_document().validated_contract().unwrap();
    for minor in [9u16, 10] {
        let mut bytes = source.clone();
        bytes[6..8].copy_from_slice(&minor.to_le_bytes());
        let result = Executor::new(&contract, compiler_limits())
            .unwrap()
            .run_graph(
                "reader-graph",
                vec![Value::Bytes(bytes.into()), Value::Integer(0)],
            );
        assert_eq!(
            matches!(result, Ok(ref values) if values == &vec![Value::Integer(source.len() as i128)]),
            minor == 10
        );
    }
}

#[test]
fn g0_raw_parser_checks_unsigned_word_codec_minor_version() {
    let document = compiler_document();
    let graph = document
        .graphs
        .iter()
        .find(|graph| graph.name == "reader-u32-codec")
        .unwrap();
    let source = g0::graph_binary::encode_graph(graph).unwrap();
    let contract = document.validated_contract().unwrap();
    let mut executor = Executor::new(&contract, compiler_limits()).unwrap();
    for minor in [10u16, 11] {
        let mut bytes = source.clone();
        bytes[6..8].copy_from_slice(&minor.to_le_bytes());
        let result = executor.run_graph(
            "reader-graph",
            vec![Value::Bytes(bytes.into()), Value::Integer(0)],
        );
        assert_eq!(
            matches!(result, Ok(ref values) if values == &vec![Value::Integer(source.len() as i128)]),
            minor == 11
        );
    }
}

#[test]
fn g0_semantic_compiler_reads_its_own_definition_with_bounded_hosting() {
    let source = g0::program_binary::encode_program(&compiler_document()).unwrap();
    let assembly = compile_with(&source, &source).unwrap();
    assert_eq!(assembly.matches(".byte ").count(), source.len());
}

#[test]
fn g0_port_list_reader_rejects_duplicate_identifiers() {
    let contract = compiler_document().validated_contract().unwrap();
    for (ids, valid) in [([0u16, 17], true), ([17, 0], true), ([17, 17], false)] {
        let mut source = 2u32.to_le_bytes().to_vec();
        for (index, id) in ids.into_iter().enumerate() {
            source.extend_from_slice(&id.to_le_bytes());
            source.extend_from_slice(&1u32.to_le_bytes());
            source.push(b'a' + index as u8);
            source.push(0);
        }
        let length = source.len() as i128;
        let output = Executor::new(&contract, compiler_limits())
            .unwrap()
            .run_graph(
                "reader-list-reader-port",
                vec![Value::Bytes(source.into()), Value::Integer(0)],
            );
        assert_eq!(
            matches!(output,Ok(ref values) if values==&vec![Value::Integer(length)]),
            valid
        );
    }
}

#[test]
fn g0_graph_reader_rejects_duplicate_node_identifiers() {
    let mut graph = g0::editor::GraphEditor::new().graph().clone();
    let mut second = graph.nodes[0].clone();
    second.id = 2;
    graph.nodes.push(second);
    let mut source = g0::graph_binary::encode_graph(&graph).unwrap();
    let contract = compiler_document().validated_contract().unwrap();
    let output = Executor::new(&contract, compiler_limits())
        .unwrap()
        .run_graph(
            "reader-graph-ast",
            vec![Value::Bytes(source.clone().into()), Value::Integer(0)],
        )
        .unwrap();
    let [Value::Array(nodes)] = output.as_slice() else {
        panic!("nodes")
    };
    let Value::Array(fields) = &nodes[1] else {
        panic!("node")
    };
    let Value::Integer(offset) = fields[1] else {
        panic!("node offset")
    };
    source[offset as usize..offset as usize + 4].copy_from_slice(&1u32.to_le_bytes());
    let length = source.len() as i128;
    let output = Executor::new(&contract, compiler_limits())
        .unwrap()
        .run_graph(
            "reader-graph",
            vec![Value::Bytes(source.into()), Value::Integer(0)],
        );
    assert!(!matches!(output,Ok(ref values) if values==&vec![Value::Integer(length)]));
}

#[test]
fn g0_graph_reader_rejects_unknown_edge_source() {
    let graph = g0::editor::GraphEditor::new().graph().clone();
    let mut source = g0::graph_binary::encode_graph(&graph).unwrap();
    let len = source.len();
    source[len - 9..len - 5].copy_from_slice(&99u32.to_le_bytes());
    let contract = compiler_document().validated_contract().unwrap();
    let output = Executor::new(&contract, compiler_limits())
        .unwrap()
        .run_graph(
            "reader-graph",
            vec![Value::Bytes(source.into()), Value::Integer(0)],
        );
    assert!(!matches!(output,Ok(ref values) if values==&vec![Value::Integer(len as i128)]));
}

#[test]
fn g0_graph_reader_rejects_duplicate_and_missing_graph_output_edges() {
    use g0::gir::*;
    let mut graph = g0::editor::GraphEditor::new().graph().clone();
    let mut output = graph.outputs[0].clone();
    output.id = 1;
    output.name = "second".into();
    graph.outputs.push(output);
    graph.edges.push(Edge {
        from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
        to: TargetEndpoint::GraphOutput(1),
    });
    let mut source = g0::graph_binary::encode_graph(&graph).unwrap();
    let len = source.len();
    source[len - 2..].copy_from_slice(&0u16.to_le_bytes());
    let contract = compiler_document().validated_contract().unwrap();
    let output = Executor::new(&contract, compiler_limits())
        .unwrap()
        .run_graph(
            "reader-graph",
            vec![Value::Bytes(source.into()), Value::Integer(0)],
        );
    assert!(!matches!(output,Ok(ref values) if values==&vec![Value::Integer(len as i128)]));
}

#[test]
fn g0_type_assignment_checks_ranges_and_nested_shapes() {
    use g0::gir::*;
    let narrow = SemanticType::Integer(IntegerType { min: -7, max: 42 });
    let wide = SemanticType::Integer(IntegerType {
        min: i128::MIN,
        max: i128::MAX,
    });
    let cases = vec![
        (narrow.clone(), wide.clone(), true),
        (wide.clone(), narrow.clone(), false),
        (SemanticType::Bool, SemanticType::Text, false),
        (
            SemanticType::Array(Box::new(narrow.clone()), 2),
            SemanticType::Array(Box::new(wide.clone()), 2),
            true,
        ),
        (
            SemanticType::Array(Box::new(narrow.clone()), 2),
            SemanticType::Array(Box::new(wide.clone()), 3),
            false,
        ),
        (
            SemanticType::Result(
                Box::new(SemanticType::Slice(Box::new(narrow))),
                Box::new(SemanticType::Bool),
            ),
            SemanticType::Result(
                Box::new(SemanticType::Slice(Box::new(wide))),
                Box::new(SemanticType::Bool),
            ),
            true,
        ),
    ];
    let contract = compiler_document().validated_contract().unwrap();
    for (source, target, expected) in cases {
        let mut bytes = g0::graph_binary::encode_semantic_type(&source).unwrap();
        let target_at = bytes.len() as i128;
        bytes.extend(g0::graph_binary::encode_semantic_type(&target).unwrap());
        let output = Executor::new(&contract, compiler_limits())
            .unwrap()
            .run_graph(
                "validator-type-assignable",
                vec![
                    Value::Bytes(bytes.into()),
                    Value::Integer(0),
                    Value::Integer(target_at),
                ],
            )
            .unwrap();
        assert_eq!(
            output,
            vec![Value::Bool(expected)],
            "{source:?} -> {target:?}"
        );
    }
}

#[test]
fn g0_graph_reader_rejects_incompatible_edge_types() {
    let graph = g0::editor::GraphEditor::new().graph().clone();
    let mut source = g0::graph_binary::encode_graph(&graph).unwrap();
    let outputs = 8 + 4 + graph.name.len() + 1 + 4;
    let output_type = outputs + 4 + 2 + 4 + graph.outputs[0].name.len();
    assert_eq!(source[output_type], 1);
    source.splice(output_type..output_type + 33, [0]);
    let length = source.len() as i128;
    let contract = compiler_document().validated_contract().unwrap();
    let output = Executor::new(&contract, compiler_limits())
        .unwrap()
        .run_graph(
            "reader-graph",
            vec![Value::Bytes(source.into()), Value::Integer(0)],
        );
    assert!(!matches!(output,Ok(ref values) if values==&vec![Value::Integer(length)]));
}
