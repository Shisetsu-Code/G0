#[path = "support/program_fixture.rs"]
#[allow(dead_code)]
mod fixture;
use g0::{
    execution::{ExecutionLimits, RuntimeError},
    native_runtime::*,
};
#[test]
fn explicit_host_limits_do_not_come_from_embedded_program() {
    let bytes = g0::program_binary::encode_program(&fixture::call_program()).unwrap();
    assert!(execute_embedded(&bytes, &[]).is_ok());
    assert!(matches!(
        execute_embedded_with_limits(
            &bytes,
            &[],
            ExecutionLimits {
                max_steps: 1,
                ..Default::default()
            }
        ),
        Err(NativeRuntimeError::Runtime(RuntimeError::StepLimit))
    ));
    unsafe {
        let limits = NativeLimits {
            max_steps: 1,
            max_value_bytes: 64 * 1024 * 1024,
            max_call_depth: 128,
        };
        let result =
            g0_runtime_entry_with_limits(bytes.as_ptr(), bytes.len(), std::ptr::null(), 0, &limits);
        assert_eq!(g0_runtime_status(result), 1);
        assert_eq!(g0_runtime_failure_kind(result), 1);
        g0_runtime_free(result);
        let result = g0_runtime_entry_with_limits(
            bytes.as_ptr(),
            bytes.len(),
            std::ptr::null(),
            0,
            std::ptr::null(),
        );
        assert_eq!(g0_runtime_status(result), 1);
        assert_eq!(g0_runtime_failure_kind(result), 5);
        g0_runtime_free(result);
    }
}

#[test]
fn integer128_abi_preserves_both_limbs_and_rejects_short_buffers() {
    use g0::gir::{IntegerType, Literal, Operation, SemanticType};
    for value in [i128::MIN, i128::MAX, -1, 0] {
        let mut document = fixture::call_program();
        let mut graph = document.graphs[0].clone();
        graph.inputs.clear();
        graph.nodes.truncate(1);
        graph.outputs.truncate(1);
        graph.outputs[0].ty = SemanticType::Integer(IntegerType::new(value, value).unwrap());
        graph.nodes[0].operation = Operation::Const(Literal::Integer(value));
        graph.nodes[0].inputs.clear();
        graph.nodes[0].outputs = graph.outputs.clone();
        graph.edges = vec![g0::gir::Edge {
            from: g0::gir::SourceEndpoint::NodeOutput {
                node: graph.nodes[0].id,
                port: graph.outputs[0].id,
            },
            to: g0::gir::TargetEndpoint::GraphOutput(graph.outputs[0].id),
        }];
        document.entry_graph = graph.name.clone();
        document.graphs = vec![graph];
        let source = g0::program_binary::encode_program(&document).unwrap();
        unsafe {
            let result = g0_runtime_entry(source.as_ptr(), source.len(), std::ptr::null(), 0);
            assert_eq!(g0_runtime_status(result), 0);
            assert_eq!(g0_runtime_failure_kind(result), 0);
            let mut limbs = [17u64, 19u64];
            assert_ne!(g0_runtime_integer128(result, limbs.as_mut_ptr(), 1), 0);
            assert_eq!(limbs, [17, 19]);
            assert_eq!(g0_runtime_integer128(result, limbs.as_mut_ptr(), 2), 0);
            assert_eq!(
                (u128::from(limbs[1]) << 64 | u128::from(limbs[0])) as i128,
                value
            );
            g0_runtime_free(result);
        }
    }
}

#[test]
fn failure_diagnostics_distinguish_memory_and_invalid_handles() {
    let bytes = g0::program_binary::encode_program(&fixture::call_program()).unwrap();
    unsafe {
        assert_eq!(g0_runtime_failure_kind(std::ptr::null()), 7);
        let limits = NativeLimits {
            max_steps: 100,
            max_value_bytes: 1,
            max_call_depth: 128,
        };
        let result =
            g0_runtime_entry_with_limits(bytes.as_ptr(), bytes.len(), std::ptr::null(), 0, &limits);
        assert_eq!(g0_runtime_failure_kind(result), 2);
        g0_runtime_free(result);
    }
}

#[test]
fn explicit_compiler_reservation_remains_bounded_and_does_not_change_defaults() {
    let limits = NativeLimits {
        max_steps: 64_000_000,
        max_value_bytes: 32 * 1024 * 1024 * 1024,
        max_call_depth: 128,
    };
    assert_eq!(
        limits.execution_limits().unwrap().max_value_bytes,
        limits.max_value_bytes
    );
    assert!(
        NativeLimits {
            max_steps: limits.max_steps + 1,
            ..limits
        }
        .execution_limits()
        .is_err()
    );
    assert!(
        NativeLimits {
            max_value_bytes: limits.max_value_bytes + 1,
            ..limits
        }
        .execution_limits()
        .is_err()
    );
    assert_eq!(ExecutionLimits::default().max_value_bytes, 64 * 1024 * 1024);
    assert_eq!(ExecutionLimits::default().max_steps, 1_000_000);
}

#[test]
fn host_accepts_large_valid_documents_and_rejects_the_declared_source_ceiling() {
    use g0::gir::{Edge, Literal, Operation, SemanticType, SourceEndpoint, TargetEndpoint};
    let mut document = fixture::call_program();
    let mut graph = document.graphs.remove(0);
    graph.inputs.clear();
    graph.outputs[0].ty = SemanticType::Text;
    graph.nodes.truncate(1);
    graph.nodes[0].operation = Operation::Const(Literal::Text("x".repeat(1024 * 1024)));
    graph.nodes[0].inputs.clear();
    graph.nodes[0].outputs = graph.outputs.clone();
    graph.edges = vec![Edge {
        from: SourceEndpoint::NodeOutput {
            node: graph.nodes[0].id,
            port: graph.outputs[0].id,
        },
        to: TargetEndpoint::GraphOutput(graph.outputs[0].id),
    }];
    document.entry_graph = graph.name.clone();
    document.graphs = vec![graph];
    let source = g0::program_binary::encode_program(&document).unwrap();
    assert!(source.len() > 1024 * 1024);
    let output = execute_embedded(&source, &[]).unwrap();
    assert!(matches!(&output[..], [g0::value::Value::Text(text)] if text.len() == 1024 * 1024));
    let oversized = vec![0; g0::bootstrap_compiler::MAX_SOURCE_BYTES + 1];
    assert!(matches!(
        execute_embedded(&oversized, &[]),
        Err(NativeRuntimeError::TooLarge)
    ));
}
