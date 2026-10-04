use g0::{
    execution::{Executor, RuntimeError},
    gir::*,
    program::ProgramContract,
    value::Value,
};

fn full() -> SemanticType {
    SemanticType::Integer(IntegerType {
        min: 0,
        max: u32::MAX as i128,
    })
}
fn program() -> ProgramContract {
    let input = Port {
        id: 0,
        name: "bytes".into(),
        ty: SemanticType::Bytes,
    };
    let output = Port {
        id: 0,
        name: "integer".into(),
        ty: full(),
    };
    let mut graph = Graph::new("decode");
    graph.inputs = vec![input.clone()];
    graph.outputs = vec![output.clone()];
    graph.nodes = vec![Node {
        id: 1,
        operation: Operation::DecodeUnsigned32Le,
        inputs: vec![input],
        outputs: vec![output],
        effects: Default::default(),
        required_capabilities: Default::default(),
    }];
    graph.edges = vec![
        Edge {
            from: SourceEndpoint::GraphInput(0),
            to: TargetEndpoint::NodeInput { node: 1, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    ProgramContract {
        graphs: vec![graph],
        entry_graph: Some("decode".into()),
        ..Default::default()
    }
}

#[test]
fn every_bit_and_unsigned_extremes_decode_identically_in_both_runtimes() {
    let p = program();
    let values = [0, u32::MAX, 0x80000000, 0x01020304]
        .into_iter()
        .chain((0..32).map(|bit| 1u32 << bit));
    for n in values {
        let value = Value::Bytes(n.to_le_bytes().to_vec().into());
        assert_eq!(
            Executor::new(&p, Default::default())
                .unwrap()
                .run_graph("decode", vec![value.clone()])
                .unwrap(),
            vec![Value::Integer(i128::from(n))]
        );
        let mut context =
            g0::native_aggregate_runtime::NativeContext::new(p.clone(), Default::default())
                .unwrap();
        let input = context.insert_value(value).unwrap();
        let output = context.primitive(0, 0, &[input]);
        assert_eq!(context.value(output), Some(&Value::Integer(i128::from(n))));
    }
}
#[test]
fn malformed_length_is_a_bounded_decode_failure() {
    let p = program();
    for len in [0, 1, 3, 5, 8] {
        let input = Value::Bytes(vec![0; len].into());
        assert!(matches!(
            Executor::new(&p, Default::default())
                .unwrap()
                .run_graph("decode", vec![input.clone()]),
            Err(RuntimeError::Bounds { .. })
        ));
        let mut c = g0::native_aggregate_runtime::NativeContext::new(p.clone(), Default::default())
            .unwrap();
        let h = c.insert_value(input).unwrap();
        assert_eq!(c.primitive(0, 0, &[h]), 0);
        assert!(matches!(c.error, Some(RuntimeError::Bounds { .. })));
    }
}
#[test]
fn version_eleven_roundtrips_and_older_versions_reject_opcode() {
    let graph = program().graphs.remove(0);
    let bytes = g0::graph_binary::encode_graph(&graph).unwrap();
    assert_eq!(&bytes[6..8], &11u16.to_le_bytes());
    assert_eq!(
        g0::graph_binary_decode::decode_graph(&bytes).unwrap(),
        graph
    );
    for minor in 1u16..11 {
        let mut legacy = bytes.clone();
        legacy[6..8].copy_from_slice(&minor.to_le_bytes());
        assert!(g0::graph_binary_decode::decode_graph(&legacy).is_err());
    }
}
#[test]
fn narrow_outputs_are_rejected_and_editor_forms_preserve_full_width() {
    let mut p = program();
    p.graphs[0].nodes[0].outputs[0].ty = SemanticType::Integer(IntegerType { min: 0, max: 255 });
    p.graphs[0].outputs[0].ty = p.graphs[0].nodes[0].outputs[0].ty.clone();
    assert!(g0::composite::validate_node(&p.graphs[0].nodes[0], None).is_err());
    assert!(Executor::new(&p, Default::default()).is_err());
    let mut editor = g0::editor::GraphEditor::new();
    let id = editor.add_operation(Operation::DecodeUnsigned32Le).unwrap();
    let text = editor.node_form_text(id).unwrap();
    let mut restored = g0::editor::GraphEditor::new();
    let restored_id = restored.apply_node_form(&text).unwrap();
    let node = restored
        .graph()
        .nodes
        .iter()
        .find(|n| n.id == restored_id)
        .unwrap();
    assert_eq!(node.operation, Operation::DecodeUnsigned32Le);
    assert_eq!(node.inputs[0].ty, SemanticType::Bytes);
    assert_eq!(node.outputs[0].ty, full());
}

#[test]
fn pure_codec_rejects_effects_capabilities_wrong_arity_and_input_type() {
    for capability in [false, true] {
        let mut p = program();
        if capability {
            p.graphs[0].nodes[0]
                .required_capabilities
                .insert(Capability {
                    class: CapabilityClass::Network,
                    action: "read".into(),
                    resource: "endpoint".into(),
                    scope: "test".into(),
                });
        } else {
            p.graphs[0].nodes[0].effects.insert(Effect::MemoryWrite);
        }
        assert!(g0::gir_validate::validate(&p.graphs[0]).is_err());
        assert!(g0::native_aggregate::compile_program(&p).is_err());
        assert!(g0::native_aggregate_runtime::NativeContext::new(p, Default::default()).is_err());
    }
    let node = program().graphs.remove(0).nodes.remove(0);
    let mut wrong = node.clone();
    wrong.inputs[0].ty = SemanticType::Text;
    assert!(g0::composite::validate_node(&wrong, None).is_err());
    let mut wrong = node.clone();
    wrong.inputs.clear();
    assert!(g0::composite::validate_node(&wrong, None).is_err());
    let mut wrong = node;
    wrong.outputs.push(wrong.outputs[0].clone());
    assert!(g0::composite::validate_node(&wrong, None).is_err());
}

#[test]
fn codec_dispatch_obeys_cancellation_and_step_budget() {
    let p = program();
    for cancelled in [true, false] {
        let limits = g0::execution::ExecutionLimits {
            max_steps: if cancelled { 10 } else { 0 },
            ..Default::default()
        };
        let expected = if cancelled {
            RuntimeError::Cancelled
        } else {
            RuntimeError::StepLimit
        };
        let mut executor = Executor::new(&p, limits).unwrap();
        if cancelled {
            executor.cancellation().cancel();
        }
        assert_eq!(
            executor.run_graph("decode", vec![Value::Bytes(vec![0; 4].into())]),
            Err(expected.clone())
        );
        let mut context =
            g0::native_aggregate_runtime::NativeContext::new(p.clone(), limits).unwrap();
        let input = context
            .insert_value(Value::Bytes(vec![0; 4].into()))
            .unwrap();
        if cancelled {
            context.cancellation().cancel();
        }
        assert_eq!(context.primitive(0, 0, &[input]), 0);
        assert_eq!(context.error, Some(expected));
    }
}

const C_DRIVER: &str = r#"
#include <stdint.h>
#include <stddef.h>
typedef struct NativeContext NativeContext;
typedef struct NativeResult NativeResult;
typedef struct {uint64_t steps,bytes,depth;} Limits;
extern uint64_t g0_compiled_entry_with_inputs(NativeContext*,const uint64_t*,uint64_t);
extern NativeResult*g0_native_invoke(const unsigned char*,size_t,const unsigned char*,size_t,uint64_t(*)(NativeContext*,const uint64_t*,uint64_t),const Limits*);
extern int32_t g0_runtime_status(const NativeResult*);
extern int32_t g0_runtime_failure_kind(const NativeResult*);
extern int32_t g0_runtime_integer128(const NativeResult*,uint64_t*,size_t);
extern void g0_runtime_free(NativeResult*);
int main(void){
 const Limits limits={100,1048576,10};
 for(unsigned bit=0;bit<32;bit++){
  unsigned char input[4]={0}; input[bit/8]=(unsigned char)(1u<<(bit%8));
  NativeResult*r=g0_native_invoke(document,sizeof(document),input,4,g0_compiled_entry_with_inputs,&limits);
  uint64_t limbs[2]={0}; int bad=g0_runtime_status(r)||g0_runtime_integer128(r,limbs,2);
  uint64_t expected[2]={0};expected[bit/64]=UINT64_C(1)<<(bit%64);
  bad|=limbs[0]!=expected[0]||limbs[1]!=expected[1];g0_runtime_free(r);if(bad)return 1;
 }
 unsigned char input[5]; for(unsigned i=0;i<5;i++)input[i]=255;
 NativeResult*r=g0_native_invoke(document,sizeof(document),input,4,g0_compiled_entry_with_inputs,&limits);
 uint64_t limbs[2];int bad=g0_runtime_status(r)||g0_runtime_integer128(r,limbs,2)||limbs[0]!=UINT32_MAX||limbs[1]!=0;
 g0_runtime_free(r);if(bad)return 2;
 for(unsigned n=3;n<=5;n+=2){
  r=g0_native_invoke(document,sizeof(document),input,n,g0_compiled_entry_with_inputs,&limits);
  bad=!g0_runtime_status(r)||g0_runtime_failure_kind(r)!=6;g0_runtime_free(r);if(bad)return 3;
 }
 return 0;
}
"#;

fn native_fixture() -> (String, String) {
    let p = program();
    assert!(g0::native_program::requires_aggregate_values(&p.graphs[0]));
    let compiled = g0::native_aggregate::compile_program(&p).unwrap();
    assert!(compiled.assembly.contains("call g0_native_primitive"));
    let document = compiled.document;
    let values = document
        .iter()
        .map(u8::to_string)
        .collect::<Vec<_>>()
        .join(",");
    (
        compiled.assembly,
        format!("static const unsigned char document[]={{ {values} }};\n{C_DRIVER}"),
    )
}

#[test]
fn native_codec_fixture_is_generated_on_windows_too() {
    let _ = native_fixture();
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn emitted_codec_executes_through_existing_c_abi() {
    use std::process::Command;
    let (assembly, driver) = native_fixture();
    let dir = std::env::temp_dir().join(format!("g0-unsigned-codec-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("program.s"), assembly).unwrap();
    std::fs::write(dir.join("driver.c"), driver).unwrap();
    let library = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/debug/libg0.a");
    assert!(library.is_file(), "cargo build --offline --lib required");
    let output = Command::new("cc")
        .current_dir(&dir)
        .args(["program.s", "driver.c"])
        .arg(library)
        .args(["-ldl", "-lpthread", "-lm", "-o", "codec"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = Command::new(dir.join("codec")).output().unwrap();
    assert!(
        output.status.success(),
        "exit {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
