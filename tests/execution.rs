#[path = "support/program_fixture.rs"]
mod fixture;

use g0::execution::{ExecutionLimits, Executor, RuntimeError};
use g0::value::Value;

#[test]
fn executes_native_calls_branches_and_loops() {
    for (document, expected) in [
        (fixture::call_program(), 42),
        (fixture::select_program(), 42),
        (fixture::loop_program(), 0),
    ] {
        let program = document.validated_contract().unwrap();
        let mut runtime = Executor::new(&program, ExecutionLimits::default()).unwrap();
        assert_eq!(runtime.run_entry().unwrap(), vec![Value::Integer(expected)]);
        assert!(runtime.steps_used() > 0);
    }
}

#[test]
fn budgets_and_cancellation_stop_nested_execution() {
    let program = fixture::call_program().validated_contract().unwrap();
    let mut runtime = Executor::new(
        &program,
        ExecutionLimits {
            max_steps: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(runtime.run_entry(), Err(RuntimeError::StepLimit)));
    let mut runtime = Executor::new(&program, ExecutionLimits::default()).unwrap();
    runtime.cancellation().cancel();
    assert!(matches!(runtime.run_entry(), Err(RuntimeError::Cancelled)));
    let mut runtime = Executor::new(
        &program,
        ExecutionLimits {
            max_call_depth: 1,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(matches!(runtime.run_entry(), Err(RuntimeError::CallDepth)));
}

#[test]
fn input_values_are_checked_against_semantic_ranges() {
    let program = fixture::loop_program().validated_contract().unwrap();
    let mut runtime = Executor::new(&program, ExecutionLimits::default()).unwrap();
    assert!(matches!(
        runtime.run_graph("condition", vec![Value::Integer(11)]),
        Err(RuntimeError::TypeMismatch { .. })
    ));
    assert_eq!(
        runtime
            .run_graph("condition", vec![Value::Integer(7)])
            .unwrap(),
        vec![Value::Bool(true)]
    );
}

#[test]
fn cli_runs_checked_in_native_programs() {
    for (name, expected) in [
        ("call.g0p", "42"),
        ("select.g0p", "42"),
        ("loop.g0p", "0"),
        ("truncate.g0g", "31"),
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_g0c"))
            .args(["run", &format!("examples/{name}")])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), expected);
    }
}

#[test]
fn text_constants_are_bounded_before_execution_and_secrets_are_redacted() {
    let mut program = fixture::call_program().validated_contract().unwrap();
    let graph = program
        .graphs
        .iter_mut()
        .find(|g| g.name == "worker")
        .unwrap();
    graph.nodes[0].operation =
        g0::gir::Operation::Const(g0::gir::Literal::Text("large payload".into()));
    graph.nodes[0].outputs[0].ty = g0::gir::SemanticType::Text;
    graph.outputs[0].ty = g0::gir::SemanticType::Text;
    program.graphs.retain(|g| g.name == "worker");
    program.entry_graph = Some("worker".into());
    let mut runtime = Executor::new(
        &program,
        ExecutionLimits {
            max_value_bytes: 4,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(runtime.run_entry(), Err(RuntimeError::MemoryLimit));
    let secret = Value::Secret(std::sync::Arc::new(Value::Text("do not print".into())));
    assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
}

#[test]
fn declared_capabilities_never_grant_authority_and_host_results_are_checked() {
    use g0::execution::EffectHost;
    use g0::gir::{Capability, CapabilityClass, Effect, Operation};
    struct Host(usize);
    impl EffectHost for Host {
        fn execute(&mut self, _: &g0::gir::Node, _: &[Value]) -> Result<Vec<Value>, RuntimeError> {
            self.0 += 1;
            Ok(vec![Value::Integer(999)])
        }
    }
    let mut program = fixture::call_program().validated_contract().unwrap();
    program.graphs.retain(|g| g.name == "worker");
    program.entry_graph = Some("worker".into());
    let node = &mut program.graphs[0].nodes[0];
    node.operation = Operation::LocalExecute("artifact".into());
    node.effects.insert(Effect::LocalExecution);
    let cap = Capability::new(
        CapabilityClass::LocalExecution,
        "execute",
        "artifact",
        "tenant-a",
    );
    node.required_capabilities.insert(cap.clone());
    let mut runtime = Executor::new(&program, ExecutionLimits::default()).unwrap();
    let mut host = Host(0);
    assert_eq!(
        runtime.run_with_host("worker", vec![], &mut host),
        Err(RuntimeError::MissingCapability(cap.clone()))
    );
    assert_eq!(host.0, 0);
    runtime.grant(cap);
    assert!(matches!(
        runtime.run_with_host("worker", vec![], &mut host),
        Err(RuntimeError::TypeMismatch { .. })
    ));
    assert_eq!(host.0, 1);
}

#[test]
fn diagnostics_and_trace_never_print_sensitive_values() {
    use g0::gir::{Graph, Literal, Node, Operation, Port, SemanticType};
    let mut graph = Graph::new("secret-constant");
    graph.nodes.push(Node {
        id: 1,
        operation: Operation::Const(Literal::Text("PRIVATE-CREDENTIAL-MARKER".into())),
        inputs: vec![],
        outputs: vec![Port {
            id: 0,
            name: "secret".into(),
            ty: SemanticType::Credential(Box::new(SemanticType::Text)),
        }],
        effects: Default::default(),
        required_capabilities: Default::default(),
    });
    assert!(
        !g0::gir_validate::validate(&graph)
            .unwrap_err()
            .to_string()
            .contains("PRIVATE-CREDENTIAL-MARKER")
    );
    graph.nodes[0].outputs[0].ty = SemanticType::Text;
    graph.nodes[0].effects.insert(g0::gir::Effect::Storage);
    graph.nodes[0]
        .required_capabilities
        .insert(g0::gir::Capability::new(
            g0::gir::CapabilityClass::Storage,
            "read",
            "r",
            "s",
        ));
    graph.nodes[0].inputs = graph.nodes[0].outputs.clone();
    assert!(
        !g0::gir_validate::validate(&graph)
            .unwrap_err()
            .to_string()
            .contains("PRIVATE-CREDENTIAL-MARKER")
    );
    let trace = g0::execution::TraceEvent {
        graph: "g".into(),
        node: 1,
        outputs: vec![Value::Credential(std::sync::Arc::new(Value::Text(
            "PRIVATE-CREDENTIAL-MARKER".into(),
        )))],
    };
    assert!(!format!("{trace:?}").contains("PRIVATE-CREDENTIAL-MARKER"));
}
