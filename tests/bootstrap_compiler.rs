use g0::{bootstrap_compiler::*, execution::RuntimeError, value::Value};

#[test]
fn compiler_graph_generates_deterministic_native_wrapper_and_compiles_itself() {
    let compiler = g0::program_binary::encode_program(&compiler_document()).unwrap();
    assert_eq!(compiler, COMPILER_SOURCE);
    let source = g0::program_binary::encode_program(&g0::program_binary::ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![g0::editor::GraphEditor::new().graph().clone()],
        schemas: vec![],
    })
    .unwrap();
    let assembly = compile_with(&compiler, &source).unwrap();
    assert!(assembly.contains("jmp g0_runtime_entry"));
    assert!(assembly.contains(&format!("movq ${}, %rsi", source.len())));
    assert_eq!(assembly.matches(".byte ").count(), source.len());
    assert_eq!(compile_with(&compiler, &source).unwrap(), assembly);
    let stage_one = compile_with(&compiler, &compiler).unwrap();
    let stage_two = g0::native_runtime::execute_embedded(&compiler, &compiler).unwrap();
    assert_eq!(stage_two, vec![Value::Text(stage_one.into())]);
}

#[test]
fn native_abi_returns_owned_results_and_rejects_bad_input_ranges() {
    use g0::native_runtime::*;
    // Valid Rust-owned slices and handles satisfy the documented FFI preconditions.
    unsafe {
        let result = g0_runtime_entry(
            COMPILER_SOURCE.as_ptr(),
            COMPILER_SOURCE.len(),
            COMPILER_SOURCE.as_ptr(),
            COMPILER_SOURCE.len(),
        );
        assert_eq!(g0_runtime_status(result), 0);
        let mut length = 0;
        let bytes = g0_runtime_bytes(result, &mut length);
        assert!(!bytes.is_null());
        assert!(length > COMPILER_SOURCE.len());
        assert_eq!(
            std::slice::from_raw_parts(bytes, length),
            compile_native(COMPILER_SOURCE).unwrap().as_bytes()
        );
        g0_runtime_free(result);
        let bad = g0_runtime_entry(std::ptr::null(), 1, std::ptr::null(), 0);
        assert_eq!(g0_runtime_status(bad), 1);
        g0_runtime_free(bad);
        assert_eq!(g0_runtime_status(std::ptr::null()), 1);
        g0_runtime_free(std::ptr::null_mut());
    }
}

#[test]
fn compiler_rejects_invalid_source_and_never_runs_source_effects() {
    let compiler = g0::program_binary::encode_program(&compiler_document()).unwrap();
    assert!(compile_with(&compiler, b"malformed").is_err());
    let mut graph = g0::editor::GraphEditor::new().graph().clone();
    graph.nodes[0].operation = g0::gir::Operation::LocalExecute("artifact".into());
    graph.nodes[0]
        .effects
        .insert(g0::gir::Effect::LocalExecution);
    graph.nodes[0]
        .required_capabilities
        .insert(g0::gir::Capability::new(
            g0::gir::CapabilityClass::LocalExecution,
            "execute",
            "artifact",
            "scope",
        ));
    // A declaration remains a definition. Compilation itself grants no authority.
    let source = g0::program_binary::encode_program(&g0::program_binary::ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![graph],
        schemas: vec![],
    })
    .unwrap();
    assert!(compile_with(&compiler, &source).is_ok());
    assert!(matches!(
        g0::native_runtime::execute_embedded(&source, &[]),
        Err(g0::native_runtime::NativeRuntimeError::Runtime(
            RuntimeError::MissingCapability(_)
        ))
    ));
}
