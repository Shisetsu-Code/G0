//! Temporary diagnostics: run compiler phases as native GIR machine code.
//! Each phase receives the same canonical compile-direct input and limits.

use g0::bootstrap_compiler::compiler_document;
use g0::gir::{
    Edge, Graph, Literal, Node, Operation, Port, SemanticType, SourceEndpoint, TargetEndpoint,
};
use g0::program_binary::{ProgramDocument, encode_program};
use std::{path::Path, process::Command};

fn port(name: &str, ty: SemanticType) -> Port {
    Port {
        id: 0,
        name: name.into(),
        ty,
    }
}

fn node(id: u32, operation: Operation, inputs: Vec<Port>, outputs: Vec<Port>) -> Node {
    Node {
        id,
        operation,
        inputs,
        outputs,
        effects: Default::default(),
        required_capabilities: Default::default(),
    }
}

fn edge(from: SourceEndpoint, to: TargetEndpoint) -> Edge {
    Edge { from, to }
}

// Convert Bool through real G0 Select, so the C driver can also reject false.
fn bool_probe(document: &mut ProgramDocument, phase: &str) -> String {
    let entry = format!("native-phase-probe-{phase}");
    for (name, text) in [
        ("native-phase-true", "true"),
        ("native-phase-false", "false"),
    ] {
        let mut branch = Graph::new(name);
        branch.outputs = vec![port("value", SemanticType::Text)];
        branch.nodes = vec![node(
            1,
            Operation::Const(Literal::Text(text.into())),
            vec![],
            branch.outputs.clone(),
        )];
        branch.edges = vec![edge(
            SourceEndpoint::NodeOutput { node: 1, port: 0 },
            TargetEndpoint::GraphOutput(0),
        )];
        document.graphs.push(branch);
    }
    let mut graph = Graph::new(&entry);
    graph.inputs = vec![port("source", SemanticType::Bytes)];
    graph.outputs = vec![port("value", SemanticType::Text)];
    graph.nodes = vec![
        node(
            1,
            Operation::Subgraph(phase.into()),
            graph.inputs.clone(),
            vec![port("valid", SemanticType::Bool)],
        ),
        node(
            2,
            Operation::Select {
                when_true: "native-phase-true".into(),
                when_false: "native-phase-false".into(),
            },
            vec![port("valid", SemanticType::Bool)],
            graph.outputs.clone(),
        ),
    ];
    graph.edges = vec![
        edge(
            SourceEndpoint::GraphInput(0),
            TargetEndpoint::NodeInput { node: 1, port: 0 },
        ),
        edge(
            SourceEndpoint::NodeOutput { node: 1, port: 0 },
            TargetEndpoint::NodeInput { node: 2, port: 0 },
        ),
        edge(
            SourceEndpoint::NodeOutput { node: 2, port: 0 },
            TargetEndpoint::GraphOutput(0),
        ),
    ];
    document.graphs.push(graph);
    entry
}

const DRIVER: &str = r#"
#include <stdint.h>
#include <stddef.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef struct NativeResult NativeResult;
typedef struct NativeContext NativeContext;
typedef struct {uint64_t max_steps,max_value_bytes,max_call_depth;} NativeLimits;
extern uint64_t g0_compiled_entry_with_inputs(NativeContext*,const uint64_t*,uint64_t);
extern NativeResult*g0_native_invoke(const unsigned char*,size_t,const unsigned char*,size_t,uint64_t(*)(NativeContext*,const uint64_t*,uint64_t),const NativeLimits*);
extern int32_t g0_runtime_status(const NativeResult*);
extern int32_t g0_runtime_failure_kind(const NativeResult*);
extern const unsigned char*g0_runtime_bytes(const NativeResult*,size_t*);
extern void g0_runtime_free(NativeResult*);
static unsigned char*read(const char*path,size_t*n){
 FILE*f=fopen(path,"rb");if(!f)return NULL;
 unsigned char*b=malloc(4194305);if(!b){fclose(f);return NULL;}
 *n=fread(b,1,4194305,f);int bad=ferror(f);fclose(f);
 if(bad||*n>4194304){free(b);return NULL;}return b;
}
int main(int argc,char**argv){
 if(argc!=5)return 1;size_t pn=0,in=0;
 unsigned char*p=read(argv[1],&pn),*input=read(argv[2],&in);
 if(!p||!input){free(p);free(input);return 2;}
 const NativeLimits limits={64000000,UINT64_C(16)*1024*1024*1024,128};
 NativeResult*r=g0_native_invoke(p,pn,input,in,g0_compiled_entry_with_inputs,&limits);
 free(p);free(input);
 if(g0_runtime_status(r)){fprintf(stderr,"phase %s failure kind %d\n",argv[3],g0_runtime_failure_kind(r));g0_runtime_free(r);return 3;}
 size_t size=0;const unsigned char*out=g0_runtime_bytes(r,&size);
 int good=out&&(strcmp(argv[4],"bool")==0 ? size==4&&!memcmp(out,"true",4) : size>=6&&!memcmp(out,".text\n",6));
 if(!good)fprintf(stderr,"phase %s completed with invalid output (%zu bytes)\n",argv[3],size);
 g0_runtime_free(r);return good?0:4;
}
"#;

fn link(directory: &Path, assembly: &str, library: &Path) {
    std::fs::write(directory.join("program.s"), assembly).unwrap();
    std::fs::write(directory.join("driver.c"), DRIVER).unwrap();
    let output = Command::new("cc")
        .current_dir(directory)
        .args(["program.s", "driver.c"])
        .arg(library)
        .args(["-ldl", "-lpthread", "-lm", "-o", "probe"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_native_phases() {
    let library = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/libg0.a");
    assert!(library.is_file(), "run cargo build --offline --lib first");
    // Capture once: all four probes use a coherent source snapshot.
    let mut source_document = compiler_document();
    source_document.entry_graph = "compile-direct".into();
    source_document.validated_contract().unwrap();
    let source = encode_program(&source_document).unwrap();
    let directory = std::env::temp_dir().join(format!("g0-native-phases-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("source.g0p"), &source).unwrap();
    let mut failures = vec![];
    for phase in [
        "reader-container",
        "validator-program-names",
        "validator-program-references",
        "control-compile",
    ] {
        let mut document = source_document.clone();
        let boolean = phase != "control-compile";
        document.entry_graph = if boolean {
            bool_probe(&mut document, phase)
        } else {
            phase.into()
        };
        let contract = document.validated_contract().unwrap();
        let assembly = g0::native_aggregate::compile_program(&contract).unwrap();
        std::fs::write(
            directory.join("probe.g0p"),
            encode_program(&document).unwrap(),
        )
        .unwrap();
        link(&directory, &assembly.assembly, &library);
        let start = std::time::Instant::now();
        let output = Command::new(directory.join("probe"))
            .arg(directory.join("probe.g0p"))
            .arg(directory.join("source.g0p"))
            .arg(phase)
            .arg(if boolean { "bool" } else { "assembly" })
            .output()
            .unwrap();
        eprintln!(
            "native phase {phase}: {:?}, elapsed {:?}, source {} bytes",
            output.status.code(),
            start.elapsed(),
            source.len()
        );
        if !output.status.success() {
            failures.push(format!(
                "{phase}: exit {:?}: {}",
                output.status.code(),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    std::fs::remove_dir_all(&directory).unwrap();
    assert!(
        failures.is_empty(),
        "native phase diagnostics:\n{}",
        failures.join("\n")
    );
}

#[test]
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn native_compiler_phases_fit_the_explicit_limits() {
    run_native_phases();
}

#[test]
fn native_phase_bool_probes_have_valid_exact_interfaces() {
    // Keep the Linux harness type-checked on other platforms as well.
    let _harness = run_native_phases as fn();
    let source = compiler_document();
    for phase in [
        "reader-container",
        "validator-program-names",
        "validator-program-references",
    ] {
        let mut document = source.clone();
        document.entry_graph = bool_probe(&mut document, phase);
        document.validated_contract().unwrap();
        encode_program(&document).unwrap();
    }
}
