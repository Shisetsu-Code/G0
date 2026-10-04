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
fn full_width_integer_constants_select_the_value_backend() {
    for value in [i128::MIN, i128::MAX, i128::from(i64::MAX) + 1] {
        let mut graph = text_graph();
        graph.outputs[0].ty = SemanticType::Integer(IntegerType::new(value, value).unwrap());
        graph.nodes[0].operation = Operation::Const(Literal::Integer(value));
        graph.nodes[0].outputs = graph.outputs.clone();
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
    }
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

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn native_integer128_runs_through_the_typed_c_abi() {
    let directory = std::env::temp_dir().join(format!("g0-native-i128-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let mut graph = text_graph();
    graph.outputs[0].ty = SemanticType::Integer(IntegerType::new(i128::MIN, i128::MIN).unwrap());
    graph.nodes[0].operation = Operation::Const(Literal::Integer(i128::MIN));
    graph.nodes[0].outputs = graph.outputs.clone();
    let p = ProgramContract {
        entry_graph: Some(graph.name.clone()),
        graphs: vec![graph.clone()],
        ..Default::default()
    };
    let compiled = g0::native_program::compile_program(
        &p,
        &g0::program::PlatformContract::bootstrap_x86_64_v3(),
        g0::machine::MachineProfile::x86_64_v3(),
    )
    .unwrap();
    let document = g0::program_binary::ProgramDocument {
        entry_graph: graph.name.clone(),
        graphs: vec![graph],
        schemas: vec![],
    };
    std::fs::write(
        directory.join("program.g0p"),
        g0::program_binary::encode_program(&document).unwrap(),
    )
    .unwrap();
    std::fs::write(directory.join("program.s"), compiled.assembly).unwrap();
    std::fs::write(directory.join("driver.c"), r#"
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
typedef struct NativeResult NativeResult;
typedef uint64_t (*NativeEntry)(void *, const uint64_t *, uint64_t);
extern uint64_t g0_compiled_entry_with_inputs(void *, const uint64_t *, uint64_t);
extern NativeResult *g0_native_invoke(const unsigned char *, size_t, const unsigned char *, size_t, NativeEntry, const void *);
extern int32_t g0_runtime_status(const NativeResult *);
extern int32_t g0_runtime_integer128(const NativeResult *, uint64_t *, size_t);
extern void g0_runtime_free(NativeResult *);
int main(void) {
    unsigned char bytes[65536];
    FILE *file = fopen("program.g0p", "rb");
    if (!file) return 2;
    size_t length = fread(bytes, 1, sizeof bytes, file);
    int failed = ferror(file);
    fclose(file);
    if (failed || length == sizeof bytes) return 3;
    NativeResult *result = g0_native_invoke(bytes, length, NULL, 0, g0_compiled_entry_with_inputs, NULL);
    uint64_t limbs[2] = {17,19};
    int ok = g0_runtime_status(result) == 0
        && g0_runtime_integer128(result, limbs, 2) == 0
        && limbs[0] == 0 && limbs[1] == (UINT64_C(1) << 63);
    g0_runtime_free(result);
    return ok ? 0 : 4;
}
"#).unwrap();
    let library = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/libg0.a");
    assert!(
        library.exists(),
        "cargo build --lib must precede Linux native integration tests"
    );
    let built = std::process::Command::new("cc")
        .current_dir(&directory)
        .args(["program.s", "driver.c"])
        .arg(library)
        .args(["-ldl", "-lpthread", "-lm", "-o", "program"])
        .output()
        .unwrap();
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stderr)
    );
    assert!(
        std::process::Command::new(directory.join("program"))
            .current_dir(&directory)
            .status()
            .unwrap()
            .success()
    );
    for name in ["program.g0p", "program.s", "driver.c", "program"] {
        std::fs::remove_file(directory.join(name)).unwrap();
    }
    std::fs::remove_dir(directory).unwrap();
}
