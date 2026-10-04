use g0::{gir::*, program::ProgramContract};
fn text_graph() -> Graph {
    let mut graph = Graph::new("text-entry");
    graph.outputs = vec![Port {
        id: 0,
        name: "text".into(),
        ty: SemanticType::Text,
    }];
    graph.nodes.push(Node {
        id: 1,
        operation: Operation::Const(Literal::Text("native aggregate".into())),
        inputs: vec![],
        outputs: graph.outputs.clone(),
        effects: Default::default(),
        required_capabilities: Default::default(),
    });
    graph.edges.push(Edge {
        from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
        to: TargetEndpoint::GraphOutput(0),
    });
    graph
}
#[test]
fn program_compilation_selects_direct_aggregate_backend() {
    let graph = text_graph();
    let p = ProgramContract {
        entry_graph: Some(graph.name.clone()),
        graphs: vec![graph],
        ..Default::default()
    };
    let compiled = g0::native_program::compile_program(
        &p,
        &g0::program::PlatformContract::bootstrap_x86_64_v3(),
        g0::machine::MachineProfile::x86_64_v3(),
    )
    .unwrap();
    assert!(compiled.assembly.contains("call g0_native_primitive"));
    assert!(!compiled.assembly.contains("g0_runtime_entry"));
}
#[test]
fn cli_compiles_aggregate_graph_and_preserves_invalid_output() {
    let directory = std::env::temp_dir().join(format!("g0-aggregate-cli-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("text.g0g");
    let output = directory.join("text.s");
    std::fs::write(
        &source,
        g0::graph_binary::encode_graph(&text_graph()).unwrap(),
    )
    .unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_g0c"))
        .arg("compile")
        .arg(&source)
        .arg("-o")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let assembly = std::fs::read(&output).unwrap();
    assert!(String::from_utf8_lossy(&assembly).contains("g0_native_primitive"));
    std::fs::write(&source, b"G0G\0broken").unwrap();
    assert!(
        !std::process::Command::new(env!("CARGO_BIN_EXE_g0c"))
            .arg("compile")
            .arg(&source)
            .arg("-o")
            .arg(&output)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(std::fs::read(&output).unwrap(), assembly);
    std::fs::remove_file(source).unwrap();
    std::fs::remove_file(output).unwrap();
    std::fs::remove_dir(directory).unwrap();
}
