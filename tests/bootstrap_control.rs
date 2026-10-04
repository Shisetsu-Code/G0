use g0::{
    bootstrap_compiler::{compiler_document, compiler_limits},
    execution::Executor,
    gir::{IntegerType, SemanticType},
    value::Value,
};
#[path = "support/program_fixture.rs"]
mod fixture;
fn values(ns: &[i128]) -> Value {
    Value::Array(
        ns.iter()
            .map(|v| Value::Integer(*v))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn run(name: &str, args: Vec<Value>) -> Vec<Value> {
    let p = compiler_document().validated_contract().unwrap();
    Executor::new(&p, compiler_limits())
        .unwrap()
        .run_graph(name, args)
        .unwrap()
}
fn port_list(ids: &[u16]) -> Vec<u8> {
    let mut b = (ids.len() as u32).to_le_bytes().to_vec();
    for id in ids {
        b.extend_from_slice(&id.to_le_bytes());
        b.extend_from_slice(&0_u32.to_le_bytes());
        b.extend(
            g0::graph_binary::encode_semantic_type(&SemanticType::Integer(IntegerType {
                min: 0,
                max: 100,
            }))
            .unwrap(),
        )
    }
    b
}

#[test]
fn g0_control_port_rank_maps_sparse_ids_to_abi_ordinals() {
    let source = port_list(&[2, 20, 99]);
    for (id, rank) in [(2, 0), (20, 1), (99, 2)] {
        assert_eq!(
            run(
                "control-port-rank",
                vec![
                    Value::Bytes(source.clone().into()),
                    Value::Integer(0),
                    Value::Integer(id)
                ]
            ),
            vec![Value::Integer(rank)]
        )
    }
}
#[test]
fn g0_control_flattens_multiple_node_outputs_without_id_arithmetic() {
    let mut source = port_list(&[3, 9]);
    let second = source.len();
    source.extend(port_list(&[42]));
    let descriptors = Value::Array(
        vec![
            values(&[7, 0, 0, 0, 0, 0, 0, 0]),
            values(&[90, 0, 0, 0, second as i128, 0, 0, 0]),
        ]
        .into(),
    );
    let output = run(
        "control-node-slots",
        vec![Value::Bytes(source.into()), descriptors],
    );
    assert_eq!(
        output,
        vec![
            Value::Integer(3),
            Value::Array(vec![values(&[7, 3, 0]), values(&[7, 9, 1]), values(&[90, 42, 2])].into())
        ]
    );
}
#[test]
fn g0_control_symbols_are_injective_for_utf8_names() {
    for (name, symbol) in [
        ("ab", "g0_direct_name_97_98_"),
        ("é", "g0_direct_name_195_169_"),
    ] {
        let mut source = (name.len() as u32).to_le_bytes().to_vec();
        source.extend_from_slice(name.as_bytes());
        assert_eq!(
            run(
                "control-symbol",
                vec![Value::Bytes(source.into()), Value::Integer(0)]
            ),
            vec![Value::Text(symbol.into())]
        )
    }
}

#[test]
fn g0_control_arguments_use_port_rank_and_flattened_slots() {
    let mut source = port_list(&[2, 20]);
    let target_ports = source.len() as i128;
    source.extend(port_list(&[5, 99]));
    let node = values(&[80, 0, 0, target_ports, 0, 0, 0, 0]);
    let slots = Value::Array(vec![values(&[7, 42, 3])].into());
    let edges = Value::Array(
        vec![
            values(&[1, 7, 42, 0, 80, 99]),
            values(&[0, 0, 20, 0, 80, 5]),
            values(&[1, 7, 42, 0, 81, 5]),
        ]
        .into(),
    );
    let result = run(
        "control-arguments",
        vec![
            Value::Bytes(source.into()),
            node,
            slots,
            edges,
            Value::Integer(0),
        ],
    );
    assert_eq!(
        result,
        vec![Value::Text(
            concat!(
                "    movq 24(%r14), %rax\n    movq %rax, 8(%r15)\n",
                "    movq 8(%r13), %rax\n    movq %rax, 0(%r15)\n"
            )
            .into()
        )]
    );
}

#[test]
fn g0_control_emits_native_subgraph_and_select_calls() {
    for (tag, names) in [(7, vec!["é"]), (4, vec!["yes", "no"])] {
        let mut source = vec![tag];
        for name in names {
            source.extend_from_slice(&(name.len() as u32).to_le_bytes());
            source.extend_from_slice(name.as_bytes());
        }
        let ports = source.len() as i128;
        source.extend(port_list(&[2, 99]));
        let descriptor = values(&[70, 0, 0, ports, ports, 0, 0, 0]);
        let output = run(
            "control-operation",
            vec![
                Value::Bytes(source.into()),
                Value::Integer(3),
                Value::Integer(8),
                descriptor,
            ],
        );
        let Value::Text(code) = &output[0] else {
            panic!()
        };
        assert!(code.contains("call g0_native_control_begin"));
        assert!(code.contains("movq $3, %rsi"));
        assert!(code.contains("movq $8, %rdx"));
        assert!(code.contains(".Lg0_control_3_fail"));
        if tag == 7 {
            assert!(code.contains("call g0_direct_name_195_169_"));
        } else {
            assert!(code.contains("call g0_native_truth"));
            assert!(code.contains("call g0_direct_name_121_101_115_"));
            assert!(code.contains("call g0_direct_name_110_111_"));
        }
    }
}

#[test]
fn g0_control_outputs_unpack_typed_packs_into_sparse_node_slots() {
    let source = port_list(&[3, 99]);
    let descriptor = values(&[70, 0, 0, 0, 0, 0, 0, 0]);
    let slots = Value::Array(vec![values(&[70, 3, 2]), values(&[70, 99, 7])].into());
    let output = run(
        "control-store-outputs",
        vec![
            Value::Bytes(source.into()),
            Value::Integer(3),
            Value::Integer(8),
            descriptor,
            slots,
        ],
    );
    let Value::Text(code) = &output[0] else {
        panic!()
    };
    assert!(code.contains("call g0_native_pack_check"));
    assert_eq!(code.matches("call g0_native_pack_item").count(), 2);
    assert!(code.contains("movq %rax, 16(%r14)"));
    assert!(code.contains("movq %rax, 56(%r14)"));
}

#[test]
fn g0_control_loop_preserves_parallel_state_and_checks_full_u64_bound() {
    let mut source = vec![6];
    for name in ["cond", "body"] {
        source.extend_from_slice(&(name.len() as u32).to_le_bytes());
        source.extend_from_slice(name.as_bytes());
    }
    source.extend_from_slice(&u64::MAX.to_le_bytes());
    let ports = source.len() as i128;
    source.extend(port_list(&[4, 9]));
    let output = run(
        "control-operation",
        vec![
            Value::Bytes(source.into()),
            Value::Integer(3),
            Value::Integer(8),
            values(&[70, 0, 0, ports, ports, 0, 0, 0]),
        ],
    );
    let Value::Text(code) = &output[0] else {
        panic!()
    };
    assert!(code.contains("movabsq $18446744073709551615, %rax"));
    assert!(code.contains("call g0_native_loop_fail"));
    assert!(code.contains("call g0_native_tick"));
    assert_eq!(code.matches("call g0_native_pack_item").count(), 2);
    assert!(code.contains("movq %rax, 0(%r15)"));
    assert!(code.contains("movq %rax, 8(%r15)"));
    assert!(code.contains("call g0_native_pack\n"));
}

#[test]
fn g0_control_map_and_match_emit_direct_body_dispatch() {
    let mut map = vec![47];
    map.extend_from_slice(&4u32.to_le_bytes());
    map.extend_from_slice(b"body");
    let map_ports = map.len() as i128;
    map.extend(port_list(&[99]));
    let output = run(
        "control-operation",
        vec![
            Value::Bytes(map.into()),
            Value::Integer(3),
            Value::Integer(8),
            values(&[70, 0, 0, map_ports, map_ports, 0, 0, 0]),
        ],
    );
    let Value::Text(code) = &output[0] else {
        panic!()
    };
    for symbol in [
        "g0_native_sequence_len",
        "g0_native_sequence_item",
        "g0_native_map_begin",
        "g0_native_map_push",
        "g0_native_map_finish",
        "g0_direct_name_98_111_100_121_",
    ] {
        assert!(code.contains(&format!("call {symbol}")), "{symbol}");
    }
    let mut source = vec![5];
    source.extend_from_slice(&2u32.to_le_bytes());
    for name in ["red", "yes", "blue", "no", "fallback"] {
        source.extend_from_slice(&(name.len() as u32).to_le_bytes());
        source.extend_from_slice(name.as_bytes());
    }
    let ports = source.len() as i128;
    source.extend(port_list(&[5, 20]));
    let output = run(
        "control-operation",
        vec![
            Value::Bytes(source.into()),
            Value::Integer(3),
            Value::Integer(8),
            values(&[70, 0, 0, ports, ports, 0, 0, 0]),
        ],
    );
    let Value::Text(code) = &output[0] else {
        panic!()
    };
    assert!(code.contains("call g0_native_match_arm"));
    assert!(code.contains("cmpq $0, %rax"));
    assert!(code.contains("cmpq $1, %rax"));
    assert!(code.contains("call g0_direct_name_121_101_115_"));
    assert!(code.contains("call g0_direct_name_110_111_"));
    assert!(code.contains("call g0_direct_name_102_97_108_108_98_97_99_107_"));
}

#[test]
fn g0_control_compiles_multiple_graphs_with_dynamic_frames() {
    let mut graph = g0::editor::GraphEditor::new().graph().clone();
    graph.name = "é".into();
    let source = g0::program_binary::encode_program(&g0::program_binary::ProgramDocument {
        entry_graph: "é".into(),
        graphs: vec![graph],
        schemas: vec![],
    })
    .unwrap();
    let output = run("control-compile", vec![Value::Bytes(source.clone().into())]);
    let Value::Text(code) = &output[0] else {
        panic!()
    };
    assert!(code.contains("g0_direct_name_195_169_:"));
    assert!(code.contains("call g0_native_graph_enter"));
    assert!(code.contains("call g0_native_graph_result"));
    assert!(code.contains("call g0_native_leave"));
    assert!(code.contains("jmp g0_native_invoke"));
    assert!(code.find("call g0_native_graph_enter").unwrap() < code.find("subq $").unwrap());
    assert_eq!(code.matches(".byte ").count(), source.len());
}

#[test]
fn g0_control_domain_accepts_control_graphs_and_rejects_effect_operations() {
    for document in [
        fixture::call_program(),
        fixture::select_program(),
        fixture::loop_program(),
    ] {
        let source = g0::program_binary::encode_program(&document).unwrap();
        assert_eq!(
            run("control-domain", vec![Value::Bytes(source.into())]),
            vec![Value::Bool(true)]
        );
    }
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
    let source = g0::program_binary::encode_program(&g0::program_binary::ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![graph],
        schemas: vec![],
    })
    .unwrap();
    assert_eq!(
        run("control-domain", vec![Value::Bytes(source.into())]),
        vec![Value::Bool(false)]
    );
}

#[test]
fn g0_control_domain_checks_nested_port_type_profiles() {
    use g0::gir::{IntegerType, Port, SemanticType};
    let integer = SemanticType::Integer(IntegerType::new(0, 100).unwrap());
    for (ty, accepted) in [
        (SemanticType::Array(Box::new(integer.clone()), 2), true),
        (
            SemanticType::Result(Box::new(SemanticType::Bytes), Box::new(SemanticType::Bool)),
            true,
        ),
        (
            SemanticType::Option(Box::new(SemanticType::Unique(Box::new(integer)))),
            false,
        ),
        (
            SemanticType::Slice(Box::new(SemanticType::Shared(Box::new(
                SemanticType::Bytes,
            )))),
            false,
        ),
        (SemanticType::Secret(Box::new(SemanticType::Bytes)), false),
    ] {
        let mut graph = g0::editor::GraphEditor::new().graph().clone();
        graph.inputs.push(Port {
            id: 0,
            name: "profile".into(),
            ty,
        });
        let source = g0::program_binary::encode_program(&g0::program_binary::ProgramDocument {
            entry_graph: "main".into(),
            graphs: vec![graph],
            schemas: vec![],
        })
        .unwrap();
        assert_eq!(
            run("control-domain", vec![Value::Bytes(source.into())]),
            vec![Value::Bool(accepted)]
        );
    }
}

fn sparse(
    mut document: g0::program_binary::ProgramDocument,
) -> g0::program_binary::ProgramDocument {
    use g0::gir::*;
    for graph in &mut document.graphs {
        for p in graph.inputs.iter_mut().chain(graph.outputs.iter_mut()) {
            p.id = p.id * 11 + 5;
        }
        for node in &mut graph.nodes {
            node.id = node.id * 7 + 3;
            for p in node.inputs.iter_mut().chain(node.outputs.iter_mut()) {
                p.id = p.id * 11 + 5;
            }
        }
        for edge in &mut graph.edges {
            match &mut edge.from {
                SourceEndpoint::GraphInput(p) => *p = *p * 11 + 5,
                SourceEndpoint::NodeOutput { node, port } => {
                    *node = *node * 7 + 3;
                    *port = *port * 11 + 5;
                }
            }
            match &mut edge.to {
                TargetEndpoint::GraphOutput(p) => *p = *p * 11 + 5,
                TargetEndpoint::NodeInput { node, port } => {
                    *node = *node * 7 + 3;
                    *port = *port * 11 + 5;
                }
            }
        }
    }
    document
}

#[test]
fn g0_control_full_program_emission_handles_sparse_multigraph_control() {
    for document in [
        fixture::call_program(),
        fixture::select_program(),
        fixture::loop_program(),
    ] {
        let document = sparse(document);
        document.validated_contract().unwrap();
        let source = g0::program_binary::encode_program(&document).unwrap();
        let output = run("control-compile", vec![Value::Bytes(source.into())]);
        let Value::Text(code) = &output[0] else {
            panic!()
        };
        assert_eq!(
            code.matches("call g0_native_graph_enter").count(),
            document.graphs.len()
        );
        assert!(code.contains("call g0_native_control_begin"));
        assert!(!code.contains("g0_runtime_entry"));
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn link(directory: &std::path::Path, assembly: &str, host: &str, executable: &str) {
    use std::process::Command;
    std::fs::write(directory.join("program.s"), assembly).unwrap();
    std::fs::write(directory.join("host.c"), host).unwrap();
    let library = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/libg0.a");
    assert!(library.is_file(), "run cargo build --offline --lib first");
    let output = Command::new("cc")
        .current_dir(directory)
        .args(["program.s", "host.c"])
        .arg(library)
        .args(["-ldl", "-lpthread", "-lm", "-o", executable])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn g0_control_native_programs_execute_sparse_call_select_and_loop() {
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("g0-control-fixtures-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for (document, expected) in [
        (fixture::call_program(), 42),
        (fixture::select_program(), 42),
        (fixture::loop_program(), 0),
    ] {
        let source = g0::program_binary::encode_program(&sparse(document)).unwrap();
        let output = run("control-compile", vec![Value::Bytes(source.into())]);
        let Value::Text(assembly) = &output[0] else {
            panic!()
        };
        let host = format!(
            r#"
#include <stdint.h>
#include <stddef.h>
typedef struct NativeResult NativeResult;
typedef struct {{ uint64_t max_steps,max_value_bytes,max_call_depth; }} NativeLimits;
extern NativeResult*g0_compiled_entry(const unsigned char*,size_t);
extern NativeResult*g0_compiled_entry_with_limits(const unsigned char*,size_t,const NativeLimits*);
extern int32_t g0_runtime_status(const NativeResult*);
extern int32_t g0_runtime_integer(const NativeResult*,int64_t*);
extern void g0_runtime_free(NativeResult*);
int main(void){{NativeResult*r=g0_compiled_entry(NULL,0);int64_t value=0;int bad=g0_runtime_status(r)||g0_runtime_integer(r,&value)||value!={expected};g0_runtime_free(r);if(bad)return 1;const NativeLimits limits={{1,1048576,128}};r=g0_compiled_entry_with_limits(NULL,0,&limits);bad=!g0_runtime_status(r);g0_runtime_free(r);return bad?2:0;}}
"#
        );
        link(&dir, assembly, &host, "program");
        let output = Command::new(dir.join("program")).output().unwrap();
        assert!(
            output.status.success(),
            "exit {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn g0_control_compiler_selfhosts_through_two_native_generation_stages() {
    use std::process::Command;
    let dir = std::env::temp_dir().join(format!("g0-control-selfhost-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut document = compiler_document();
    document.entry_graph = "compile-direct".into();
    let compiler = g0::program_binary::encode_program(&document).unwrap();
    assert!(
        compiler.len() <= g0::bootstrap_compiler::MAX_SOURCE_BYTES,
        "compiler must fit its declared input protocol"
    );
    std::fs::write(dir.join("compiler.g0p"), &compiler).unwrap();
    let bootstrap =
        g0::native_aggregate::compile_program(&document.validated_contract().unwrap()).unwrap();
    // Rust only bootstraps the native machine code for the GIR compiler. Both
    // generation stages below execute G0 algorithms for parsing and emission.
    let stage_zero_host = r#"
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
int main(int argc,char**argv){
 if(argc!=2)return 1;FILE*f=fopen(argv[1],"rb");if(!f)return 2;
 unsigned char*b=malloc(4194305);if(!b){fclose(f);return 3;}size_t n=fread(b,1,4194305,f);int bad=ferror(f);fclose(f);if(bad||n>4194304){free(b);return 4;}
 const NativeLimits limits={64000000,UINT64_C(16)*1024*1024*1024,128};
 NativeResult*r=g0_native_invoke(b,n,b,n,g0_compiled_entry_with_inputs,&limits);free(b);
 if(g0_runtime_status(r)){fprintf(stderr,"native compiler failure kind %d\n",g0_runtime_failure_kind(r));g0_runtime_free(r);return 5;}size_t size=0;const unsigned char*out=g0_runtime_bytes(r,&size);if(!out||size<6||memcmp(out,".text\n",6)){g0_runtime_free(r);return 6;}bad=fwrite(out,1,size,stdout)!=size;g0_runtime_free(r);return bad?7:0;
}
"#;
    link(&dir, &bootstrap.assembly, stage_zero_host, "bootstrap");
    let generated = Command::new(dir.join("bootstrap"))
        .arg(dir.join("compiler.g0p"))
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "bootstrap exit {:?}: {}",
        generated.status.code(),
        String::from_utf8_lossy(&generated.stderr)
    );
    let stage_one = String::from_utf8(generated.stdout).unwrap();
    assert!(stage_one.contains("call g0_native_primitive"));
    assert!(stage_one.contains("call g0_native_pack_item"));
    assert!(!stage_one.contains("g0_runtime_entry"));
    link(
        &dir,
        &stage_one,
        include_str!("support/bootstrap_driver.c"),
        "stage-one",
    );
    let generated = Command::new(dir.join("stage-one"))
        .arg(dir.join("compiler.g0p"))
        .output()
        .unwrap();
    assert!(
        generated.status.success(),
        "stage-one exit {:?}: {}",
        generated.status.code(),
        String::from_utf8_lossy(&generated.stderr)
    );
    assert_eq!(
        generated.stdout,
        stage_one.as_bytes(),
        "G0 native generation stages must agree byte-for-byte"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
