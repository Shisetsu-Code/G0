use g0::{
    bootstrap_compiler::{compiler_document, compiler_limits},
    execution::Executor,
    gir::*,
    program_binary::{ProgramDocument, encode_program},
    value::Value,
};

fn source(n: usize) -> Vec<u8> {
    let mut graphs = vec![];
    for i in 0..n {
        let mut graph = Graph::new(format!("g{i:03}"));
        graph.outputs = vec![Port {
            id: 0,
            name: "value".into(),
            ty: SemanticType::Text,
        }];
        graph.nodes = vec![Node {
            id: 1,
            operation: if i + 1 < n {
                Operation::Subgraph(format!("g{:03}", i + 1))
            } else {
                Operation::Const(Literal::Text("done".into()))
            },
            inputs: vec![],
            outputs: graph.outputs.clone(),
            effects: Default::default(),
            required_capabilities: Default::default(),
        }];
        graph.edges = vec![Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        }];
        graphs.push(graph);
    }
    encode_program(&ProgramDocument {
        entry_graph: "g000".into(),
        graphs,
        schemas: vec![],
    })
    .unwrap()
}
fn replace_target(source: &mut [u8], old: &[u8], new: &[u8]) {
    let mut pattern = vec![7];
    pattern.extend_from_slice(&(old.len() as u32).to_le_bytes());
    pattern.extend(old);
    let at = source
        .windows(pattern.len())
        .position(|w| w == pattern)
        .unwrap()
        + 5;
    source[at..at + new.len()].copy_from_slice(new);
}
fn check(source: Vec<u8>, steps: u64) -> bool {
    let document = compiler_document();
    let contract = document.validated_contract().unwrap();
    let mut limits = compiler_limits();
    limits.max_steps = steps;
    let args = vec![Value::Bytes(source.into())];
    let result = Executor::new(&contract, limits)
        .unwrap()
        .run_graph("validator-program-callcycles", args.clone())
        .unwrap();
    assert_eq!(
        Executor::new(&contract, limits)
            .unwrap()
            .run_graph("validator-program-callcycles-fast", args)
            .unwrap(),
        result
    );
    let [Value::Bool(ok)] = result.as_slice() else {
        panic!()
    };
    *ok
}
#[test]
fn callcycles_accepts_long_dag_with_bounded_steps() {
    assert!(check(source(64), 800_000));
}
#[test]
fn callcycles_rejects_self_mutual_and_missing_targets() {
    let mut self_call = source(2);
    replace_target(&mut self_call, b"g001", b"g000");
    assert!(!check(self_call, 100_000));
    let mut mutual = source(3);
    replace_target(&mut mutual, b"g002", b"g000");
    assert!(!check(mutual, 100_000));
    let mut missing = source(2);
    replace_target(&mut missing, b"g001", b"gone");
    assert!(!check(missing, 100_000));
    let mut disconnected = g0::program_binary::decode_program(&source(3)).unwrap();
    disconnected.graphs[0].nodes[0].operation =
        Operation::Const(Literal::Text("independent".into()));
    let mut disconnected = encode_program(&disconnected).unwrap();
    replace_target(&mut disconnected, b"g002", b"g001");
    assert!(!check(disconnected, 100_000));
}

#[test]
fn callcycles_accepts_shared_black_targets_from_select() {
    let mut document = g0::program_binary::decode_program(&source(2)).unwrap();
    let graph = &mut document.graphs[0];
    let selector = Port {
        id: 0,
        name: "condition".into(),
        ty: SemanticType::Bool,
    };
    graph.nodes = vec![
        Node {
            id: 1,
            operation: Operation::Const(Literal::Bool(true)),
            inputs: vec![],
            outputs: vec![selector.clone()],
            effects: Default::default(),
            required_capabilities: Default::default(),
        },
        Node {
            id: 2,
            operation: Operation::Select {
                when_true: "g001".into(),
                when_false: "g001".into(),
            },
            inputs: vec![selector],
            outputs: graph.outputs.clone(),
            effects: Default::default(),
            required_capabilities: Default::default(),
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
    assert!(check(encode_program(&document).unwrap(), 100_000));
}

#[test]
fn callcycles_extracts_all_control_targets_and_optional_match_default() {
    fn string(bytes: &mut Vec<u8>, s: &str) -> usize {
        let at = bytes.len();
        bytes.extend_from_slice(&(s.len() as u32).to_le_bytes());
        bytes.extend_from_slice(s.as_bytes());
        at
    }
    let document = compiler_document();
    let contract = document.validated_contract().unwrap();
    for (tag, names, expected) in [
        (7, vec!["one"], vec![0]),
        (47, vec!["two"], vec![1]),
        (4, vec!["one", "two"], vec![0, 1]),
        (6, vec!["two", "one"], vec![1, 0]),
        (5, vec!["one", "two", "one"], vec![0, 1, 0]),
        (5, vec!["one", "two", ""], vec![0, 1, 2]),
    ] {
        let mut bytes = vec![];
        let mut rows = vec![];
        for name in ["one", "two"] {
            let at = string(&mut bytes, name);
            rows.push(Value::Array(
                vec![
                    Value::Integer(0),
                    Value::Integer(0),
                    Value::Integer(at as i128),
                ]
                .into(),
            ));
        }
        let operation = bytes.len();
        bytes.push(tag);
        if tag == 5 {
            bytes.extend_from_slice(&2u32.to_le_bytes());
            for (i, name) in names[..2].iter().enumerate() {
                string(&mut bytes, &format!("arm{i}"));
                string(&mut bytes, name);
            }
            string(&mut bytes, names[2]);
        } else {
            for name in names {
                string(&mut bytes, name);
            }
        }
        let result = Executor::new(&contract, compiler_limits())
            .unwrap()
            .run_graph(
                "callcycles-ref-single",
                vec![
                    Value::Bytes(bytes.into()),
                    Value::Array(rows.into()),
                    Value::Integer(operation as i128),
                ],
            )
            .unwrap();
        assert_eq!(
            result,
            vec![Value::Array(
                expected
                    .into_iter()
                    .map(Value::Integer)
                    .collect::<Vec<_>>()
                    .into()
            )]
        );
    }
}

fn native_probe_document() -> ProgramDocument {
    let mut document = compiler_document();
    let output = Port {
        id: 0,
        name: "value".into(),
        ty: SemanticType::Integer(IntegerType::new(0, 1).unwrap()),
    };
    for (name, value) in [("cycle-probe-true", 1), ("cycle-probe-false", 0)] {
        let mut graph = Graph::new(name);
        graph.outputs = vec![output.clone()];
        graph.nodes = vec![Node {
            id: 1,
            operation: Operation::Const(Literal::Integer(value)),
            inputs: vec![],
            outputs: graph.outputs.clone(),
            effects: Default::default(),
            required_capabilities: Default::default(),
        }];
        graph.edges = vec![Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        }];
        document.graphs.push(graph);
    }
    let mut graph = Graph::new("cycle-probe-main");
    graph.inputs = vec![Port {
        id: 0,
        name: "source".into(),
        ty: SemanticType::Bytes,
    }];
    graph.outputs = vec![output];
    let boolean = Port {
        id: 0,
        name: "valid".into(),
        ty: SemanticType::Bool,
    };
    graph.nodes = vec![
        Node {
            id: 1,
            operation: Operation::Subgraph("validator-program-callcycles".into()),
            inputs: graph.inputs.clone(),
            outputs: vec![boolean.clone()],
            effects: Default::default(),
            required_capabilities: Default::default(),
        },
        Node {
            id: 2,
            operation: Operation::Select {
                when_true: "cycle-probe-true".into(),
                when_false: "cycle-probe-false".into(),
            },
            inputs: vec![boolean],
            outputs: graph.outputs.clone(),
            effects: Default::default(),
            required_capabilities: Default::default(),
        },
    ];
    graph.edges = vec![
        Edge {
            from: SourceEndpoint::GraphInput(0),
            to: TargetEndpoint::NodeInput { node: 1, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::NodeInput { node: 2, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    document.graphs.push(graph);
    document.entry_graph = "cycle-probe-main".into();
    document
}

#[test]
fn callcycles_native_probe_has_valid_exact_interfaces() {
    let _harness = run_native_cycle_probe as fn();
    native_probe_document().validated_contract().unwrap();
}

fn run_native_cycle_probe() {
    use std::{path::Path, process::Command};
    let document = native_probe_document();
    let contract = document.validated_contract().unwrap();
    let assembly = g0::native_aggregate::compile_program(&contract).unwrap();
    let directory =
        std::env::temp_dir().join(format!("g0-callcycles-native-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("program.s"), assembly.assembly).unwrap();
    std::fs::write(
        directory.join("program.g0p"),
        encode_program(&document).unwrap(),
    )
    .unwrap();
    std::fs::write(directory.join("driver.c"),r#"
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
typedef struct NativeResult NativeResult;
typedef struct NativeContext NativeContext;
typedef struct {uint64_t max_steps,max_value_bytes,max_call_depth;} NativeLimits;
extern uint64_t g0_compiled_entry_with_inputs(NativeContext*,const uint64_t*,uint64_t);
extern NativeResult*g0_native_invoke(const unsigned char*,size_t,const unsigned char*,size_t,uint64_t(*)(NativeContext*,const uint64_t*,uint64_t),const NativeLimits*);
extern int32_t g0_runtime_status(const NativeResult*);
extern int32_t g0_runtime_failure_kind(const NativeResult*);
extern int32_t g0_runtime_integer(const NativeResult*,int64_t*);
extern void g0_runtime_free(NativeResult*);
static unsigned char*read(const char*name,size_t*n){FILE*f=fopen(name,"rb");if(!f)return NULL;unsigned char*b=malloc(4194305);if(!b){fclose(f);return NULL;}*n=fread(b,1,4194305,f);int bad=ferror(f);fclose(f);if(bad||*n>4194304){free(b);return NULL;}return b;}
int main(int argc,char**argv){if(argc!=4)return 1;size_t pn=0,in=0;unsigned char*p=read(argv[1],&pn),*b=read(argv[2],&in);if(!p||!b){free(p);free(b);return 2;}const NativeLimits limits={64000000,UINT64_C(16)*1024*1024*1024,128};NativeResult*r=g0_native_invoke(p,pn,b,in,g0_compiled_entry_with_inputs,&limits);free(p);free(b);if(g0_runtime_status(r)){fprintf(stderr,"cycle probe runtime failure kind %d\n",g0_runtime_failure_kind(r));g0_runtime_free(r);return 3;}int64_t value=-1;int bad=g0_runtime_integer(r,&value)||value!=atoi(argv[3]);g0_runtime_free(r);return bad?4:0;}
"#).unwrap();
    let library = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/libg0.a");
    assert!(library.is_file(), "run cargo build --offline --lib first");
    let linked = Command::new("cc")
        .current_dir(&directory)
        .args(["program.s", "driver.c"])
        .arg(library)
        .args(["-ldl", "-lpthread", "-lm", "-o", "probe"])
        .output()
        .unwrap();
    assert!(
        linked.status.success(),
        "{}",
        String::from_utf8_lossy(&linked.stderr)
    );
    let mut self_call = source(2);
    replace_target(&mut self_call, b"g001", b"g000");
    let mut mutual = source(3);
    replace_target(&mut mutual, b"g002", b"g000");
    let mut missing = source(2);
    replace_target(&mut missing, b"g001", b"gone");
    for (bytes, expected) in [(source(64), 1), (self_call, 0), (mutual, 0), (missing, 0)] {
        std::fs::write(directory.join("input.g0p"), bytes).unwrap();
        let result = Command::new(directory.join("probe"))
            .arg(directory.join("program.g0p"))
            .arg(directory.join("input.g0p"))
            .arg(expected.to_string())
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "exit {:?}: {}",
            result.status.code(),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn callcycles_native_machine_code_rejects_raw_cycles() {
    run_native_cycle_probe();
}
