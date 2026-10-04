use g0::{
    execution::{ExecutionLimits, RuntimeError},
    gir::*,
    native_aggregate,
    native_aggregate_runtime::NativeContext,
    program::ProgramContract,
    value::Value,
};
use std::collections::BTreeSet;

fn int() -> SemanticType {
    SemanticType::Integer(IntegerType::new(-100, 100).unwrap())
}
fn port(id: u16, ty: SemanticType) -> Port {
    Port {
        id,
        name: format!("p{id}"),
        ty,
    }
}
fn node(id: u32, operation: Operation, inputs: Vec<SemanticType>, output: SemanticType) -> Node {
    Node {
        id,
        operation,
        inputs: inputs
            .into_iter()
            .enumerate()
            .map(|(i, t)| port(i as u16, t))
            .collect(),
        outputs: vec![port(0, output)],
        effects: BTreeSet::new(),
        required_capabilities: BTreeSet::new(),
    }
}
fn edge(from: u32, to: u32, port: u16) -> Edge {
    Edge {
        from: SourceEndpoint::NodeOutput {
            node: from,
            port: 0,
        },
        to: TargetEndpoint::NodeInput { node: to, port },
    }
}
fn fixture(index: i128) -> ProgramContract {
    let array = SemanticType::Array(Box::new(int()), 2);
    let option = SemanticType::Option(Box::new(int()));
    let mut graph = Graph::new("main");
    graph.outputs = vec![port(0, int())];
    graph.nodes = vec![
        node(1, Operation::Const(Literal::Integer(20)), vec![], int()),
        node(2, Operation::Const(Literal::Integer(22)), vec![], int()),
        node(3, Operation::MakeArray, vec![int(), int()], array.clone()),
        node(4, Operation::Const(Literal::Integer(index)), vec![], int()),
        node(5, Operation::Index, vec![array, int()], option.clone()),
        node(6, Operation::Const(Literal::Integer(42)), vec![], int()),
        node(7, Operation::UnwrapOr, vec![option, int()], int()),
    ];
    graph.edges = vec![
        edge(1, 3, 0),
        edge(2, 3, 1),
        edge(3, 5, 0),
        edge(4, 5, 1),
        edge(5, 7, 0),
        edge(6, 7, 1),
        Edge {
            from: SourceEndpoint::NodeOutput { node: 7, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    ProgramContract {
        entry_graph: Some("main".into()),
        graphs: vec![graph],
        ..ProgramContract::default()
    }
}
fn execute_primitives(index: i128, limits: ExecutionLimits) -> (NativeContext, u64) {
    let mut c = NativeContext::new(fixture(index), limits).unwrap();
    let a = c.primitive(0, 0, &[]);
    let b = c.primitive(0, 1, &[]);
    let array = c.primitive(0, 2, &[a, b]);
    let i = c.primitive(0, 3, &[]);
    let option = c.primitive(0, 4, &[array, i]);
    let fallback = c.primitive(0, 5, &[]);
    let result = c.primitive(0, 6, &[option, fallback]);
    (c, result)
}

fn map_fixture() -> ProgramContract {
    let mut program = fixture(1);
    let array = SemanticType::Array(Box::new(int()), 2);
    let mut body = Graph::new("identity");
    body.inputs = vec![port(0, int())];
    body.outputs = body.inputs.clone();
    body.edges.push(Edge {
        from: SourceEndpoint::GraphInput(0),
        to: TargetEndpoint::GraphOutput(0),
    });
    let main = &mut program.graphs[0];
    main.nodes.push(node(
        8,
        Operation::Map {
            body: "identity".into(),
        },
        vec![array.clone()],
        SemanticType::Slice(Box::new(int())),
    ));
    main.nodes.iter_mut().find(|n| n.id == 5).unwrap().inputs[0].ty =
        SemanticType::Slice(Box::new(int()));
    main.edges
        .retain(|e| e.to != TargetEndpoint::NodeInput { node: 5, port: 0 });
    main.edges.extend([edge(3, 8, 0), edge(8, 5, 0)]);
    program.graphs.push(body);
    program
}
fn multistate_fixture() -> ProgramContract {
    let mut program = fixture(1);
    let array = SemanticType::Array(Box::new(int()), 2);
    let states = vec![port(0, SemanticType::Bool), port(1, array.clone())];
    let mut condition = Graph::new("condition");
    condition.inputs = states.clone();
    condition.outputs = vec![port(0, SemanticType::Bool)];
    condition.edges.push(Edge {
        from: SourceEndpoint::GraphInput(0),
        to: TargetEndpoint::GraphOutput(0),
    });
    let mut body = Graph::new("body");
    body.inputs = states.clone();
    body.outputs = states.clone();
    body.nodes.push(node(
        1,
        Operation::Const(Literal::Bool(false)),
        vec![],
        SemanticType::Bool,
    ));
    body.edges.extend([
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
        Edge {
            from: SourceEndpoint::GraphInput(1),
            to: TargetEndpoint::GraphOutput(1),
        },
    ]);
    let main = &mut program.graphs[0];
    main.nodes.push(node(
        8,
        Operation::Const(Literal::Bool(true)),
        vec![],
        SemanticType::Bool,
    ));
    let mut looping = node(
        9,
        Operation::Loop {
            condition: "condition".into(),
            body: "body".into(),
            max_iterations: 3,
        },
        vec![SemanticType::Bool, array],
        SemanticType::Bool,
    );
    looping.outputs = states;
    main.nodes.push(looping);
    main.edges
        .retain(|e| e.to != TargetEndpoint::NodeInput { node: 5, port: 0 });
    main.edges.extend([
        edge(8, 9, 0),
        edge(3, 9, 1),
        Edge {
            from: SourceEndpoint::NodeOutput { node: 9, port: 1 },
            to: TargetEndpoint::NodeInput { node: 5, port: 0 },
        },
    ]);
    program.graphs.extend([condition, body]);
    program
}
fn zero_output_fixture() -> ProgramContract {
    let mut graph = Graph::new("main");
    graph.outputs = vec![port(0, int())];
    graph.nodes.push(node(
        1,
        Operation::Const(Literal::Integer(22)),
        vec![],
        int(),
    ));
    for id in 2..=22 {
        let mut call = node(id, Operation::Subgraph("empty".into()), vec![], int());
        call.outputs.clear();
        graph.nodes.push(call)
    }
    graph.edges.push(Edge {
        from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
        to: TargetEndpoint::GraphOutput(0),
    });
    ProgramContract {
        entry_graph: Some("main".into()),
        graphs: vec![graph, Graph::new("empty")],
        ..ProgramContract::default()
    }
}

#[test]
fn native_map_and_multistate_loop_emit_machine_control_and_private_packs() {
    let mapped = native_aggregate::compile_program(&map_fixture()).unwrap();
    assert!(mapped.assembly.contains("call g0_native_map_finish"));
    assert!(mapped.assembly.contains("call g0_g_6964656e74697479"));
    let looped = native_aggregate::compile_program(&multistate_fixture()).unwrap();
    assert!(looped.assembly.contains("call g0_native_pack_check"));
    assert!(looped.assembly.contains("call g0_native_pack_item"));
}

#[test]
fn primitive_array_index_option_and_negative_index_match_semantics() {
    for (index, expected) in [(0, 20), (1, 22), (-1, 42), (99, 42)] {
        let (c, h) = execute_primitives(index, ExecutionLimits::default());
        assert_eq!(c.value(h), Some(&Value::Integer(expected)));
        assert_eq!(c.error, None)
    }
}
#[test]
fn memory_and_step_limits_are_sticky() {
    for limits in [
        ExecutionLimits {
            max_steps: 2,
            ..ExecutionLimits::default()
        },
        ExecutionLimits {
            max_value_bytes: 1,
            ..ExecutionLimits::default()
        },
    ] {
        let (c, h) = execute_primitives(0, limits);
        assert_eq!(h, 0);
        assert!(matches!(
            c.error,
            Some(RuntimeError::StepLimit | RuntimeError::MemoryLimit)
        ))
    }
}

#[test]
fn cancelled_context_stops_primitives_and_empty_graph_calls() {
    let mut c = NativeContext::new(fixture(0), ExecutionLimits::default()).unwrap();
    c.cancellation().cancel();
    assert_eq!(c.primitive(0, 0, &[]), 0);
    assert_eq!(c.error, Some(RuntimeError::Cancelled));
    let mut graph = Graph::new("empty");
    graph.inputs = vec![port(0, int())];
    graph.outputs = graph.inputs.clone();
    graph.edges.push(Edge {
        from: SourceEndpoint::GraphInput(0),
        to: TargetEndpoint::GraphOutput(0),
    });
    let program = ProgramContract {
        entry_graph: Some("empty".into()),
        graphs: vec![graph],
        ..ProgramContract::default()
    };
    let mut c = NativeContext::new(
        program,
        ExecutionLimits {
            max_steps: 2,
            ..ExecutionLimits::default()
        },
    )
    .unwrap();
    unsafe {
        assert_eq!(g0::native_aggregate_runtime::g0_native_enter(&mut c, 0), 1);
        g0::native_aggregate_runtime::g0_native_leave(&mut c);
        assert_eq!(g0::native_aggregate_runtime::g0_native_enter(&mut c, 0), 1);
        g0::native_aggregate_runtime::g0_native_leave(&mut c);
        assert_eq!(g0::native_aggregate_runtime::g0_native_enter(&mut c, 0), 0)
    };
    assert_eq!(c.error, Some(RuntimeError::StepLimit));
}

#[test]
fn native_bytes_utf8_and_result_fallback_preserve_failure_payload() {
    let result = SemanticType::Result(Box::new(SemanticType::Text), Box::new(SemanticType::Bytes));
    let mut graph = Graph::new("main");
    graph.outputs = vec![port(0, SemanticType::Text)];
    graph.nodes = vec![
        node(
            1,
            Operation::Const(Literal::Bytes(vec![255])),
            vec![],
            SemanticType::Bytes,
        ),
        node(
            2,
            Operation::DecodeUtf8,
            vec![SemanticType::Bytes],
            result.clone(),
        ),
        node(
            3,
            Operation::Const(Literal::Text("fallback".into())),
            vec![],
            SemanticType::Text,
        ),
        node(
            4,
            Operation::UnwrapOr,
            vec![result, SemanticType::Text],
            SemanticType::Text,
        ),
    ];
    graph.edges = vec![
        edge(1, 2, 0),
        edge(2, 4, 0),
        edge(3, 4, 1),
        Edge {
            from: SourceEndpoint::NodeOutput { node: 4, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    let program = ProgramContract {
        entry_graph: Some("main".into()),
        graphs: vec![graph],
        ..ProgramContract::default()
    };
    native_aggregate::compile_program(&program).unwrap();
    let mut c = NativeContext::new(program, ExecutionLimits::default()).unwrap();
    let bytes = c.primitive(0, 0, &[]);
    let decoded = c.primitive(0, 1, &[bytes]);
    assert!(
        matches!(c.value(decoded),Some(Value::Result(Err(v))) if **v==Value::Bytes(vec![255].into()))
    );
    let fallback = c.primitive(0, 2, &[]);
    let h = c.primitive(0, 3, &[decoded, fallback]);
    assert_eq!(c.value(h), Some(&Value::Text("fallback".into())));
    assert_eq!(c.error, None);
}

#[test]
fn native_record_fields_and_variant_payloads_are_schema_checked() {
    use g0::data_format::{DataSchema, FieldRequirement, SchemaField};
    for variant in [false, true] {
        let schema = DataSchema {
            name: "Payload".into(),
            version: 1,
            fields: vec![SchemaField {
                tag: 1,
                name: "answer".into(),
                ty: int(),
                requirement: FieldRequirement::Required,
            }],
        };
        let ty = if variant {
            SemanticType::Variant("Payload".into())
        } else {
            SemanticType::Record("Payload".into())
        };
        let extracted = if variant {
            SemanticType::Option(Box::new(int()))
        } else {
            int()
        };
        let op = if variant {
            Operation::MakeVariant {
                schema: "Payload".into(),
                tag: "answer".into(),
            }
        } else {
            Operation::MakeRecord {
                schema: "Payload".into(),
                fields: vec!["answer".into()],
            }
        };
        let getter = if variant {
            Operation::VariantPayload {
                tag: "answer".into(),
            }
        } else {
            Operation::Field {
                name: "answer".into(),
            }
        };
        let mut graph = Graph::new("main");
        graph.outputs = vec![port(0, extracted.clone())];
        graph.nodes = vec![
            node(1, Operation::Const(Literal::Integer(42)), vec![], int()),
            node(2, op, vec![int()], ty.clone()),
            node(3, getter, vec![ty], extracted),
        ];
        graph.edges = vec![
            edge(1, 2, 0),
            edge(2, 3, 0),
            Edge {
                from: SourceEndpoint::NodeOutput { node: 3, port: 0 },
                to: TargetEndpoint::GraphOutput(0),
            },
        ];
        let program = ProgramContract {
            entry_graph: Some("main".into()),
            graphs: vec![graph],
            schemas: vec![schema],
            ..ProgramContract::default()
        };
        native_aggregate::compile_program(&program).unwrap();
        let mut c = NativeContext::new(program, ExecutionLimits::default()).unwrap();
        let a = c.primitive(0, 0, &[]);
        let payload = c.primitive(0, 1, &[a]);
        let h = c.primitive(0, 2, &[payload]);
        let expected = if variant {
            Value::Option(Some(std::sync::Arc::new(Value::Integer(42))))
        } else {
            Value::Integer(42)
        };
        assert_eq!(c.value(h), Some(&expected));
        assert_eq!(c.error, None);
    }
}

#[test]
fn internal_result_packs_cannot_be_used_as_language_values() {
    use g0::native_aggregate_runtime::{g0_native_pack, g0_native_pack_check, g0_native_pack_item};
    let mut program = fixture(1);
    program.graphs[0].outputs.push(port(1, int()));
    program.graphs[0].edges.push(Edge {
        from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
        to: TargetEndpoint::GraphOutput(1),
    });
    let mut c = NativeContext::new(program, ExecutionLimits::default()).unwrap();
    let a = c.primitive(0, 0, &[]);
    let b = c.primitive(0, 1, &[]);
    let handles = [a, b];
    unsafe {
        let pack = g0_native_pack(&mut c, handles.as_ptr(), 2);
        assert_ne!(pack, 0);
        assert!(c.value(pack).is_none());
        assert_eq!(g0_native_pack_check(&mut c, 0, u64::MAX, pack), pack);
        assert_eq!(g0_native_pack_item(&mut c, pack, 1), b);
        assert_eq!(g0_native_pack_item(&mut c, pack, 2), 0)
    }
    assert_eq!(c.error, Some(RuntimeError::InvalidProgram));
}

#[test]
fn zero_output_graph_calls_charge_pack_metadata_and_stop_at_memory_limit() {
    use g0::native_aggregate_runtime::{g0_native_enter, g0_native_leave, g0_native_pack};
    let program = ProgramContract {
        entry_graph: Some("empty".into()),
        graphs: vec![Graph::new("empty")],
        ..ProgramContract::default()
    };
    let mut c = NativeContext::new(
        program,
        ExecutionLimits {
            max_value_bytes: 256,
            max_steps: 100,
            ..ExecutionLimits::default()
        },
    )
    .unwrap();
    let mut completed = 0;
    loop {
        let entered = unsafe { g0_native_enter(&mut c, 0) };
        if entered == 0 {
            break;
        }
        let pack = unsafe { g0_native_pack(&mut c, std::ptr::null(), 0) };
        unsafe { g0_native_leave(&mut c) };
        if pack == 0 {
            break;
        }
        completed += 1;
        assert!(completed < 10, "empty tuples bypassed memory budget")
    }
    assert!(completed > 0);
    assert_eq!(c.error, Some(RuntimeError::MemoryLimit));
    assert!(native_aggregate::compile_program(&zero_output_fixture()).is_ok());
}

#[test]
fn empty_map_builders_charge_metadata_before_allocation() {
    use g0::native_aggregate_runtime::g0_native_map_begin;
    let mut c = NativeContext::new(
        fixture(0),
        ExecutionLimits {
            max_value_bytes: 1,
            ..ExecutionLimits::default()
        },
    )
    .unwrap();
    assert_eq!(unsafe { g0_native_map_begin(&mut c, 0) }, 0);
    assert_eq!(c.error, Some(RuntimeError::MemoryLimit));
    for _ in 0..100 {
        assert_eq!(unsafe { g0_native_map_begin(&mut c, 0) }, 0)
    }
    let mut c = NativeContext::new(
        fixture(0),
        ExecutionLimits {
            max_value_bytes: 256,
            ..ExecutionLimits::default()
        },
    )
    .unwrap();
    let mut completed = 0;
    while unsafe { g0_native_map_begin(&mut c, 0) } != 0 {
        completed += 1;
        assert!(completed < 10, "empty builder table bypassed memory quota")
    }
    assert!(completed > 0);
    assert_eq!(c.error, Some(RuntimeError::MemoryLimit));
}
#[test]
fn emitted_native_program_has_direct_node_calls_and_embedded_metadata() {
    let compiled = native_aggregate::compile_program(&fixture(1)).unwrap();
    assert!(compiled.assembly.contains("call g0_native_primitive"));
    assert!(compiled.assembly.contains("g0_compiled_entry_handle"));
    assert!(!compiled.assembly.contains("execute_program"));
    assert_eq!(&compiled.document[..4], b"G0P\0")
}

#[test]
fn linear_resource_types_are_rejected_by_immutable_native_backend() {
    let ty = SemanticType::Unique(Box::new(SemanticType::Reference("g0.region".into())));
    let mut graph = Graph::new("main");
    graph.inputs = vec![port(0, ty)];
    graph.outputs = graph.inputs.clone();
    graph.edges = vec![Edge {
        from: SourceEndpoint::GraphInput(0),
        to: TargetEndpoint::GraphOutput(0),
    }];
    let program = ProgramContract {
        entry_graph: Some("main".into()),
        graphs: vec![graph],
        ..ProgramContract::default()
    };
    assert!(matches!(
        native_aggregate::compile_program(&program),
        Err(native_aggregate::AggregateCompileIssue::Unsupported { .. })
    ));
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn generated_aggregate_assembly_executes_with_bounded_runtime() {
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("g0-native-aggregate-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for program in [
        fixture(1),
        map_fixture(),
        multistate_fixture(),
        zero_output_fixture(),
    ] {
        let compiled = native_aggregate::compile_program(&program).unwrap();
        std::fs::write(dir.join("program.s"), compiled.assembly).unwrap();
        std::fs::write(dir.join("harness.c"),"#include <stdint.h>\nextern long g0_machine_main(void);\nextern void *g0_compiled_context(uint64_t,uint64_t,uint64_t);\nextern uint64_t g0_compiled_entry_handle(void*);\nextern uint64_t g0_native_failed(void*);\nextern long g0_native_result_integer(void*,uint64_t);\nextern void g0_native_context_free(void*);\nint main(void){if(g0_machine_main()!=22)return 1;void*c=g0_compiled_context(100,1048576,10);if(!c)return 2;uint64_t h=g0_compiled_entry_handle(c);int bad=g0_native_failed(c)||g0_native_result_integer(c,h)!=22;g0_native_context_free(c);if(bad)return 3;c=g0_compiled_context(1,1048576,10);h=g0_compiled_entry_handle(c);bad=h!=0||!g0_native_failed(c);g0_native_context_free(c);return bad?4:0;}\n").unwrap();
        let library = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/libg0.a");
        assert!(
            library.is_file(),
            "run cargo build --offline --lib before Linux native ABI integration tests"
        );
        let output = Command::new("cc")
            .current_dir(&dir)
            .args(["program.s", "harness.c"])
            .arg(library)
            .args(["-ldl", "-lpthread", "-lm", "-o", "program"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let output = Command::new(dir.join("program")).output().unwrap();
        assert!(
            output.status.success(),
            "native exit {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
        if program.graphs.iter().any(|g| g.name == "empty") {
            std::fs::write(dir.join("quota.c"),"#include <stdint.h>\nextern void*g0_compiled_context(uint64_t,uint64_t,uint64_t);extern uint64_t g0_compiled_entry_handle(void*);extern uint64_t g0_native_failed(void*);extern void g0_native_context_free(void*);int main(void){void*c=g0_compiled_context(1000,512,10);if(!c)return 1;uint64_t h=g0_compiled_entry_handle(c);int bad=h!=0||!g0_native_failed(c);g0_native_context_free(c);return bad;}\n").unwrap();
            let output = Command::new("cc")
                .current_dir(&dir)
                .args(["program.s", "quota.c"])
                .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/libg0.a"))
                .args(["-ldl", "-lpthread", "-lm", "-o", "quota"])
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(Command::new(dir.join("quota")).status().unwrap().success());
        }
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
