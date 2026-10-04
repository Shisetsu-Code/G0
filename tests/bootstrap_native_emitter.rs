use g0::{bootstrap_compiler::*, execution::Executor, program_binary::*, value::Value};
#[test]
fn g0_emitter_lowers_constant_graph_to_granular_native_calls() {
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
        .run_graph("compile-direct", vec![Value::Bytes(source.into())])
        .unwrap();
    let [Value::Text(assembly)] = output.as_slice() else {
        panic!("assembly")
    };
    assert!(assembly.contains("call g0_native_primitive"), "{assembly}");
    assert!(assembly.contains("g0_direct_entry"));
    assert!(assembly.contains("jmp g0_native_invoke"));
    assert!(!assembly.contains("g0_runtime_entry"));
}

fn arithmetic_source() -> Vec<u8> {
    use g0::gir::*;
    let scalar = |n| SemanticType::Integer(IntegerType { min: n, max: n });
    let port = |id, n| Port {
        id,
        name: format!("p{id}"),
        ty: scalar(n),
    };
    let node = |id, operation, inputs, outputs| Node {
        id,
        operation,
        inputs,
        outputs,
        effects: Default::default(),
        required_capabilities: Default::default(),
    };
    let mut graph = Graph::new("main");
    graph.outputs = vec![port(0, 50)];
    graph.nodes = vec![
        node(
            1,
            Operation::Add,
            vec![port(0, 42), port(1, 8)],
            vec![port(0, 50)],
        ),
        node(
            2,
            Operation::Const(Literal::Integer(42)),
            vec![],
            vec![port(0, 42)],
        ),
        node(
            3,
            Operation::Const(Literal::Integer(8)),
            vec![],
            vec![port(0, 8)],
        ),
    ];
    graph.edges = vec![
        Edge {
            from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
            to: TargetEndpoint::NodeInput { node: 1, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 3, port: 0 },
            to: TargetEndpoint::NodeInput { node: 1, port: 1 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    encode_program(&ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![graph],
        schemas: vec![],
    })
    .unwrap()
}

#[test]
fn g0_native_emitter_schedules_and_wires_reversed_node_ids() {
    let assembly = compile_direct_native(&arithmetic_source()).unwrap();
    let constant_two = assembly.find("movq $1, %rdx").unwrap();
    let constant_three = assembly.find("movq $2, %rdx").unwrap();
    let add = assembly.find("movq $0, %rdx").unwrap();
    assert!(constant_two < constant_three && constant_three < add);
    assert_eq!(assembly.matches("call g0_native_primitive").count(), 3);
    assert!(assembly.contains("movq 8(%r14), %rax\n    movq %rax, 0(%r15)"));
    assert!(assembly.contains("movq 16(%r14), %rax\n    movq %rax, 8(%r15)"));
}

#[test]
fn g0_direct_emitter_rejects_its_unsupported_control_domain() {
    let source = encode_program(&compiler_document()).unwrap();
    assert!(matches!(
        compile_direct_native(&source),
        Err(BootstrapError::Output)
    ));
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn g0_generated_native_assembly_executes_arithmetic_and_honors_host_budgets() {
    use std::process::Command;
    let directory =
        std::env::temp_dir().join(format!("g0-gir-native-emitter-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    // This assembly is produced by executing the GIR compiler, including its
    // binary reader, AST construction, scheduling and argument wiring.
    let assembly = compile_direct_native(&arithmetic_source()).unwrap();
    std::fs::write(directory.join("program.s"), assembly).unwrap();
    std::fs::write(directory.join("harness.c"),r#"
#include <stdint.h>
#include <stddef.h>
typedef struct NativeResult NativeResult;
typedef struct { uint64_t max_steps, max_value_bytes, max_call_depth; } NativeLimits;
extern NativeResult *g0_compiled_entry(const unsigned char *, size_t);
extern NativeResult *g0_compiled_entry_with_limits(const unsigned char *, size_t, const NativeLimits *);
extern int32_t g0_runtime_status(const NativeResult *);
extern int32_t g0_runtime_integer(const NativeResult *, int64_t *);
extern void g0_runtime_free(NativeResult *);
int main(void) {
    NativeResult *result=g0_compiled_entry(NULL,0);
    if (g0_runtime_status(result)) { g0_runtime_free(result); return 1; }
    int64_t value=0;
    int failed=g0_runtime_integer(result,&value);
    g0_runtime_free(result);
    if (failed || value!=50) return 2;
    const NativeLimits limits={1,1048576,8};
    result=g0_compiled_entry_with_limits(NULL,0,&limits);
    int unexpectedly_succeeded=!g0_runtime_status(result);
    g0_runtime_free(result);
    return unexpectedly_succeeded ? 3 : 0;
}
"#).unwrap();
    let library = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/libg0.a");
    assert!(
        library.is_file(),
        "run cargo build --offline --lib before Linux native ABI integration tests"
    );
    let output = Command::new("cc")
        .current_dir(&directory)
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
    let output = Command::new(directory.join("program")).output().unwrap();
    assert!(
        output.status.success(),
        "native exit {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(directory).unwrap();
}
