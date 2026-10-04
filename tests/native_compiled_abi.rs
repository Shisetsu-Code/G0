use g0::{gir::*, native_aggregate_runtime::NativeContext, native_runtime::*};
unsafe extern "C" fn echo(_context: *mut NativeContext, handles: *const u64, count: u64) -> u64 {
    assert_eq!(count, 1);
    unsafe { *handles }
}
#[test]
fn native_compiled_entry_imports_typed_input_and_returns_owned_checked_output() {
    let mut graph = Graph::new("echo");
    graph.inputs = vec![Port {
        id: 0,
        name: "input".into(),
        ty: SemanticType::Bytes,
    }];
    graph.outputs = graph.inputs.clone();
    graph.edges = vec![Edge {
        from: SourceEndpoint::GraphInput(0),
        to: TargetEndpoint::GraphOutput(0),
    }];
    let program = g0::program_binary::encode_program(&g0::program_binary::ProgramDocument {
        entry_graph: "echo".into(),
        graphs: vec![graph],
        schemas: vec![],
    })
    .unwrap();
    let input = b"native byte input";
    unsafe {
        let result = g0::native_compiled_abi::g0_native_invoke(
            program.as_ptr(),
            program.len(),
            input.as_ptr(),
            input.len(),
            Some(echo),
            std::ptr::null(),
        );
        assert_eq!(g0_runtime_status(result), 0);
        let mut size = 0;
        let bytes = g0_runtime_bytes(result, &mut size);
        assert_eq!(std::slice::from_raw_parts(bytes, size), input);
        g0_runtime_free(result);
        let result = g0::native_compiled_abi::g0_native_invoke(
            program.as_ptr(),
            program.len(),
            input.as_ptr(),
            input.len(),
            None,
            std::ptr::null(),
        );
        assert_eq!(g0_runtime_status(result), 1);
        g0_runtime_free(result);
        let limits = NativeLimits {
            max_steps: 10,
            max_value_bytes: 1,
            max_call_depth: 128,
        };
        let result = g0::native_compiled_abi::g0_native_invoke(
            program.as_ptr(),
            program.len(),
            input.as_ptr(),
            input.len(),
            Some(echo),
            &limits,
        );
        assert_eq!(g0_runtime_status(result), 1);
        g0_runtime_free(result);
    }
}
