use g0::{
    gir::*, native_aggregate_runtime::*, native_compiled_abi::*, native_runtime::*, value::Value,
};
use std::ptr::{null, null_mut};

fn document(input: Option<SemanticType>) -> Vec<u8> {
    let ty = SemanticType::Integer(IntegerType { min: 7, max: 7 });
    let port = Port {
        id: 0,
        name: "result".into(),
        ty,
    };
    let mut graph = Graph::new("entry");
    if let Some(ty) = input {
        graph.inputs.push(Port {
            id: 0,
            name: "input".into(),
            ty,
        });
    }
    graph.outputs.push(port.clone());
    graph.nodes.push(Node {
        id: 42,
        operation: Operation::Const(Literal::Integer(7)),
        inputs: vec![],
        outputs: vec![port],
        effects: Default::default(),
        required_capabilities: Default::default(),
    });
    graph.edges.push(Edge {
        from: SourceEndpoint::NodeOutput { node: 42, port: 0 },
        to: TargetEndpoint::GraphOutput(0),
    });
    g0::program_binary::encode_program(&g0::program_binary::ProgramDocument {
        entry_graph: "entry".into(),
        graphs: vec![graph],
        schemas: vec![],
    })
    .unwrap()
}
unsafe extern "C" fn repeat(ctx: *mut NativeContext, _: *const u64, _: u64) -> u64 {
    unsafe {
        let first = g0_native_primitive(ctx, 0, 0, null(), 0);
        let second = g0_native_primitive(ctx, 0, 0, null(), 0);
        if second != 0 {
            assert_eq!(first, second);
        }
        first
    }
}
unsafe extern "C" fn retained(ctx: *mut NativeContext, _: *const u64, _: u64) -> u64 {
    unsafe {
        assert_ne!(g0_native_pack(ctx, null(), 0), 0);
        assert_ne!(g0_native_map_begin(ctx, 0), 0);
        g0_native_primitive(ctx, 0, 0, null(), 0)
    }
}
unsafe extern "C" fn insertion_failure(ctx: *mut NativeContext, _: *const u64, _: u64) -> u64 {
    unsafe {
        assert!(
            (*ctx)
                .insert_value(Value::Bytes(vec![0; 4096].into()))
                .is_err()
        );
    }
    0
}
unsafe fn invoke(
    source: &[u8],
    input: &[u8],
    entry: Option<NativeEntry>,
    limits: &NativeLimits,
) -> *mut NativeResult {
    unsafe {
        g0_native_invoke(
            source.as_ptr(),
            source.len(),
            input.as_ptr(),
            input.len(),
            entry,
            limits,
        )
    }
}
fn limits(steps: u64, bytes: u64) -> NativeLimits {
    NativeLimits {
        max_steps: steps,
        max_value_bytes: bytes,
        max_call_depth: 128,
    }
}

#[test]
fn successful_and_step_limited_entries_preserve_exact_counters() {
    let source = document(None);
    for (steps, expected, failure) in [(10, 2, 0), (1, 1, 1)] {
        unsafe {
            let result = invoke(&source, &[], Some(repeat), &limits(steps, 4096));
            let mut metrics = NativeMetrics::default();
            assert_eq!(g0_runtime_metrics(result, &mut metrics), 0);
            assert_eq!(g0_runtime_failure_kind(result), failure);
            assert_eq!(metrics.steps, expected);
            assert_eq!(metrics.value_handles, 1);
            assert_eq!(
                metrics.logical_bytes,
                expected * std::mem::size_of::<Value>() as u64
            );
            assert_eq!((metrics.pack_handles, metrics.builder_slots), (0, 0));
            g0_runtime_free(result);
        }
    }
}

#[test]
fn private_pack_and_empty_builder_slots_are_visible() {
    unsafe {
        let result = invoke(&document(None), &[], Some(retained), &limits(10, 4096));
        let mut metrics = NativeMetrics::default();
        assert_eq!(g0_runtime_metrics(result, &mut metrics), 0);
        assert_eq!(g0_runtime_status(result), 0);
        assert_eq!(
            (
                metrics.steps,
                metrics.value_handles,
                metrics.pack_handles,
                metrics.builder_slots
            ),
            (1, 1, 1, 1)
        );
        assert!(metrics.logical_bytes > std::mem::size_of::<Value>() as u64);
        g0_runtime_free(result);
    }
}

#[test]
fn unavailable_and_null_queries_do_not_modify_destination() {
    unsafe {
        let result = invoke(&document(None), &[], None, &limits(10, 4096));
        let mut metrics = NativeMetrics {
            steps: 999,
            ..Default::default()
        };
        assert_eq!(g0_runtime_metrics(result, &mut metrics), 1);
        assert_eq!(g0_runtime_metrics(null(), &mut metrics), 1);
        assert_eq!(g0_runtime_metrics(result, null_mut()), 1);
        assert_eq!(metrics.steps, 999);
        g0_runtime_free(result);
        let result = g0_runtime_entry(null(), 0, null(), 0);
        assert_eq!(g0_runtime_metrics(result, &mut metrics), 1);
        g0_runtime_free(result);
    }
}

#[test]
fn failed_insertion_preserves_prior_input_counters() {
    let input = b"input";
    unsafe {
        let result = invoke(
            &document(Some(SemanticType::Bytes)),
            input,
            Some(insertion_failure),
            &limits(10, 1024),
        );
        let mut metrics = NativeMetrics {
            steps: 999,
            ..Default::default()
        };
        assert_eq!(g0_runtime_failure_kind(result), 2);
        assert_eq!(g0_runtime_metrics(result, &mut metrics), 0);
        assert_eq!(metrics.steps, 0);
        assert_eq!(metrics.value_handles, 1);
        assert_eq!(
            metrics.logical_bytes,
            input.len() as u64 + std::mem::size_of::<Value>() as u64
        );
        g0_runtime_free(result);
    }
}
