use g0::{bootstrap_compiler::*, execution::Executor, program_binary::*, value::Value};
#[path = "support/program_fixture.rs"]
mod fixture;

#[test]
fn g0_name_comparison_forwards_only_the_name_bytes_to_its_loop() {
    use g0::gir::Operation;
    let mut source = vec![0u8; 1024 * 1024];
    source[0..4].copy_from_slice(&128u32.to_le_bytes());
    source[4..132].fill(b'a');
    source[132..136].copy_from_slice(&128u32.to_le_bytes());
    source[136..264].fill(b'a');
    let args = vec![
        Value::Bytes(source.into()),
        Value::Integer(0),
        Value::Integer(132),
    ];
    let mut limits = compiler_limits();
    limits.max_value_bytes = 64 * 1024 * 1024;
    let document = compiler_document();
    let contract = document.validated_contract().unwrap();
    let mut compact_executor = Executor::new(&contract, limits).unwrap();
    assert_eq!(
        compact_executor
            .run_graph("validator-name-equal", args.clone())
            .unwrap(),
        vec![Value::Bool(true)]
    );
    let mut baseline = document;
    let graph = baseline
        .graphs
        .iter_mut()
        .find(|g| g.name == "validator-name-equal")
        .unwrap();
    for node in &mut graph.nodes {
        if let Operation::Select { when_true, .. } = &mut node.operation
            && when_true == "validator-name-content-equal"
        {
            *when_true = "validator-byte-equal".into();
        }
    }
    let contract = baseline.validated_contract().unwrap();
    let mut baseline_executor = Executor::new(&contract, limits).unwrap();
    let baseline_result = baseline_executor.run_graph("validator-name-equal", args);
    assert_eq!(baseline_result, Ok(vec![Value::Bool(true)]));
    assert!(
        compact_executor.value_bytes_used() < baseline_executor.value_bytes_used(),
        "compact {}, baseline {}",
        compact_executor.value_bytes_used(),
        baseline_executor.value_bytes_used()
    );
}

#[test]
fn g0_native_entry_rejects_an_interface_the_byte_bridge_cannot_invoke() {
    use g0::gir::*;
    let integer = SemanticType::Integer(IntegerType { min: 0, max: 42 });
    let port = |id| Port {
        id,
        name: format!("p{id}"),
        ty: integer.clone(),
    };
    let mut graph = Graph::new("main");
    graph.inputs = vec![port(0), port(1)];
    graph.outputs = vec![port(0)];
    graph.edges = vec![Edge {
        from: SourceEndpoint::GraphInput(0),
        to: TargetEndpoint::GraphOutput(0),
    }];
    let source = encode_program(&ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![graph],
        schemas: vec![],
    })
    .unwrap();
    let compiler = compiler_document().validated_contract().unwrap();
    let output = Executor::new(&compiler, compiler_limits())
        .unwrap()
        .run_graph("compile-direct", vec![Value::Bytes(source.into())])
        .unwrap();
    let [Value::Text(output)] = output.as_slice() else {
        panic!("compiler output")
    };
    assert!(
        output.starts_with("G0 compiler:"),
        "unusable assembly emitted"
    );
}

#[test]
fn g0_program_references_reject_missing_call_target_without_host_decoder() {
    let compiler = compiler_document().validated_contract().unwrap();
    for document in [fixture::select_program(), fixture::loop_program()] {
        let source = encode_program(&document).unwrap();
        let output = Executor::new(&compiler, compiler_limits())
            .unwrap()
            .run_graph(
                "validator-program-references",
                vec![Value::Bytes(source.into())],
            )
            .unwrap();
        assert_eq!(output, vec![Value::Bool(true)]);
    }
    let mut source = encode_program(&fixture::call_program()).unwrap();
    for expected in [true, false] {
        if !expected {
            let at = source.windows(6).position(|b| b == b"worker").unwrap();
            source[at..at + 6].copy_from_slice(b"absent");
        }
        let output = Executor::new(&compiler, compiler_limits())
            .unwrap()
            .run_graph(
                "validator-program-references",
                vec![Value::Bytes(source.clone().into())],
            )
            .unwrap();
        assert_eq!(output, vec![Value::Bool(expected)]);
    }
}

#[test]
fn g0_program_names_require_distinct_graphs_and_existing_entry() {
    let compiler = compiler_document().validated_contract().unwrap();
    for (duplicate, missing_entry, expected) in [
        (false, false, true),
        (true, false, false),
        (false, true, false),
    ] {
        let first = g0::editor::GraphEditor::new().graph().clone();
        let mut second = first.clone();
        second.name = "othr".into();
        let document = ProgramDocument {
            entry_graph: "main".into(),
            graphs: vec![first, second],
            schemas: vec![],
        };
        let mut source = encode_program(&document).unwrap();
        if duplicate {
            let at = source.windows(4).position(|b| b == b"othr").unwrap();
            source[at..at + 4].copy_from_slice(b"main");
        }
        if missing_entry {
            source[12..16].copy_from_slice(b"xxxx");
        }
        let output = Executor::new(&compiler, compiler_limits())
            .unwrap()
            .run_graph("validator-program-names", vec![Value::Bytes(source.into())])
            .unwrap();
        assert_eq!(output, vec![Value::Bool(expected)]);
    }
}
