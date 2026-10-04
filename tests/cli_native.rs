use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use g0::gir::{
    Edge, Graph, IntegerType, Literal, Node, Operation, Port, SemanticType, SourceEndpoint,
    TargetEndpoint,
};

struct Workspace(PathBuf);

fn workspace_path(timestamp: u128) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "g0-cli-{}-{timestamp}-{sequence}",
        std::process::id()
    ))
}

#[test]
fn workspace_paths_remain_unique_when_clock_resolution_collides() {
    assert_ne!(workspace_path(0), workspace_path(0));
}

impl Workspace {
    fn new() -> Self {
        let path = workspace_path(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        );
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_g0c"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }

    fn write_graph(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, g0::graph_binary::encode_graph(&fixture()).unwrap()).unwrap();
        path
    }
}

impl Drop for Workspace {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture() -> Graph {
    let mut graph = Graph::new("truncate_example");
    let int = |min, max| SemanticType::Integer(IntegerType::new(min, max).unwrap());
    let output = Port {
        id: 0,
        name: "answer".into(),
        ty: int(0, 31),
    };
    graph.outputs.push(output.clone());
    graph.nodes = vec![
        Node {
            id: 1,
            operation: Operation::Const(Literal::Integer(511)),
            inputs: vec![],
            outputs: vec![Port {
                id: 0,
                name: "value".into(),
                ty: int(511, 511),
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        },
        Node {
            id: 2,
            operation: Operation::Truncate {
                bits: 5,
                signed: false,
            },
            inputs: vec![Port {
                id: 0,
                name: "value".into(),
                ty: int(0, 1000),
            }],
            outputs: vec![output],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        },
    ];
    graph.edges = vec![
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::NodeInput { node: 2, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    graph
}

fn success(output: Output) -> Output {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn check_accepts_a_canonical_graph_file() {
    let work = Workspace::new();
    work.write_graph("answer.g0g");
    let output = success(work.run(&["check", "answer.g0g"]));
    assert!(String::from_utf8_lossy(&output.stdout).contains("2 nodes"));
}

#[test]
fn compile_accepts_native_magic_with_an_arbitrary_extension() {
    let work = Workspace::new();
    work.write_graph("answer.data");
    success(work.run(&["compile", "answer.data", "-o", "build/result.s"]));
    let assembly = std::fs::read_to_string(work.0.join("build/result.s")).unwrap();
    assert!(assembly.contains("g0_machine_main:"));
    assert!(!work.0.join("answer.s").exists());
}

#[test]
fn graph_magic_takes_precedence_over_a_program_suffix() {
    let work = Workspace::new();
    work.write_graph("answer.g0p");
    success(work.run(&["check", "answer.g0p"]));
    success(work.run(&["compile", "answer.g0p"]));
    assert!(work.0.join("answer.s").is_file());
}

#[test]
fn compile_defaults_to_an_assembly_file_beside_native_input() {
    let work = Workspace::new();
    work.write_graph("answer.g0g");
    success(work.run(&["compile", "answer.g0g"]));
    assert!(work.0.join("answer.s").is_file());
}

#[test]
fn invalid_native_input_does_not_overwrite_output() {
    let work = Workspace::new();
    for bytes in [b"G0G\0".as_slice(), b"not a graph".as_slice()] {
        std::fs::write(work.0.join("bad.g0g"), bytes).unwrap();
        std::fs::write(work.0.join("result.s"), "preserve me").unwrap();
        let output = work.run(&["compile", "bad.g0g", "-o", "result.s"]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("native graph"));
        assert_eq!(
            std::fs::read_to_string(work.0.join("result.s")).unwrap(),
            "preserve me"
        );
    }
}

#[test]
fn invalid_command_arguments_do_not_write_files() {
    let work = Workspace::new();
    std::fs::write(
        work.0.join("answer.g0"),
        "g0 0.1\ntarget x86_64-v3\nconst %answer i64 42\nreturn %answer\n",
    )
    .unwrap();
    for args in [
        vec!["compile", "answer.g0", "-o"],
        vec!["compile", "answer.g0", "--typo", "result.s"],
        vec!["check", "answer.g0", "-o", "result.s"],
        vec!["compile", "answer.g0", "-o", "result.s", "extra"],
    ] {
        assert!(!work.run(&args).status.success());
        assert!(!work.0.join("answer.s").exists());
        assert!(!work.0.join("result.s").exists());
    }
}

#[test]
fn bootstrap_text_still_compiles() {
    let work = Workspace::new();
    std::fs::write(
        work.0.join("answer.g0"),
        "g0 0.1\ntarget x86_64-v3\nconst %answer i64 42\nreturn %answer\n",
    )
    .unwrap();
    success(work.run(&["check", "answer.g0"]));
    success(work.run(&["compile", "answer.g0"]));
    assert!(work.0.join("answer.s").is_file());
}

#[test]
fn compile_cannot_overwrite_its_input() {
    let work = Workspace::new();
    let input = work.write_graph("answer.g0g");
    let before = std::fs::read(&input).unwrap();
    assert!(
        !work
            .run(&["compile", "answer.g0g", "-o", "./answer.g0g"])
            .status
            .success()
    );
    assert_eq!(std::fs::read(input).unwrap(), before);
}

#[test]
fn hardlinked_output_does_not_modify_the_input_graph() {
    let work = Workspace::new();
    let input = work.write_graph("answer.g0g");
    let before = std::fs::read(&input).unwrap();
    std::fs::hard_link(&input, work.0.join("alias.s")).unwrap();
    success(work.run(&["compile", "answer.g0g", "-o", "alias.s"]));
    assert_eq!(std::fs::read(&input).unwrap(), before);
    assert!(
        std::fs::read_to_string(work.0.join("alias.s"))
            .unwrap()
            .contains("g0_machine_main:")
    );
}

#[test]
fn output_replacement_failure_leaves_no_temporary_files() {
    let work = Workspace::new();
    work.write_graph("answer.g0g");
    std::fs::create_dir(work.0.join("occupied")).unwrap();
    assert!(
        !work
            .run(&["compile", "answer.g0g", "-o", "occupied"])
            .status
            .success()
    );
    assert!(work.0.join("occupied").is_dir());
    assert_eq!(std::fs::read_dir(&work.0).unwrap().count(), 2);
}

#[test]
fn compilation_failure_preserves_existing_output() {
    let work = Workspace::new();
    let mut graph = fixture();
    graph.nodes.truncate(1);
    graph.nodes[0].operation = Operation::Const(Literal::Text("hello".into()));
    graph.nodes[0].outputs[0].ty = SemanticType::Text;
    graph.outputs[0].ty = SemanticType::Text;
    // This remains valid GIR, but the zero-argument CLI entry profile cannot
    // compile an entry with an input. Text constants themselves now compile.
    graph.inputs = vec![graph.outputs[0].clone()];
    graph.edges = vec![Edge {
        from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
        to: TargetEndpoint::GraphOutput(0),
    }];
    std::fs::write(
        work.0.join("text.g0g"),
        g0::graph_binary::encode_graph(&graph).unwrap(),
    )
    .unwrap();
    success(work.run(&["check", "text.g0g"]));
    std::fs::write(work.0.join("result.s"), "preserve me").unwrap();
    let output = work.run(&["compile", "text.g0g", "-o", "result.s"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("compilation failed"));
    assert_eq!(
        std::fs::read_to_string(work.0.join("result.s")).unwrap(),
        "preserve me"
    );
}

#[test]
fn unresolved_subgraph_is_rejected_before_emitting_assembly() {
    let work = Workspace::new();
    let mut graph = fixture();
    graph.nodes.truncate(1);
    graph.nodes[0].operation = Operation::Subgraph("missing".into());
    graph.nodes[0].outputs = graph.outputs.clone();
    graph.edges = vec![Edge {
        from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
        to: TargetEndpoint::GraphOutput(0),
    }];
    std::fs::write(
        work.0.join("missing.g0g"),
        g0::graph_binary::encode_graph(&graph).unwrap(),
    )
    .unwrap();
    for command in ["check", "compile"] {
        let output = work.run(&[command, "missing.g0g"]);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("validation failed"));
    }
    assert!(!work.0.join("missing.s").exists());
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn native_file_compiles_to_executable_code_returning_31() {
    let work = Workspace::new();
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/truncate.g0g");
    std::fs::copy(example, work.0.join("answer.g0g")).unwrap();
    success(work.run(&["compile", "answer.g0g"]));
    std::fs::write(
        work.0.join("harness.c"),
        "long g0_machine_main(void);\nint main(void) { return g0_machine_main() == 31 ? 0 : 1; }\n",
    )
    .unwrap();
    success(
        Command::new("cc")
            .current_dir(&work.0)
            .args(["answer.s", "harness.c", "-o", "answer"])
            .output()
            .unwrap(),
    );
    success(Command::new(work.0.join("answer")).output().unwrap());
}

// Keep the checked-in example consumable without running a Rust generator.
#[test]
fn checked_in_native_example_is_valid() {
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/truncate.g0g");
    assert!(input.is_file());
    let work = Workspace::new();
    success(work.run(&["check", input.to_str().unwrap()]));
}
