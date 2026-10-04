use std::collections::BTreeSet;

use g0::compiler::compile_graph;
use g0::gir::{
    Edge, Graph, IntegerType, Literal, Node, Operation, Port, SemanticType, SourceEndpoint,
    TargetEndpoint,
};
use g0::gir_optimize::optimize_gir;
use g0::machine::MachineProfile;
use g0::machine_ir::MachineOp;

fn integer(min: i128, max: i128) -> SemanticType {
    SemanticType::Integer(IntegerType::new(min, max).unwrap())
}

fn graph(bits: u16, signed: bool, constant: Option<i64>) -> Graph {
    let mut graph = Graph::new("truncate");
    let input = Port {
        id: 0,
        name: "input".into(),
        ty: integer(i64::MIN.into(), i64::MAX.into()),
    };
    let (min, max) = if signed {
        let magnitude = 1_i128 << (bits - 1);
        (-magnitude, magnitude - 1)
    } else {
        (0, (1_i128 << bits) - 1)
    };
    graph.outputs = vec![Port {
        id: 0,
        name: "result".into(),
        ty: integer(min, max),
    }];
    let source = if let Some(value) = constant {
        graph.nodes.push(Node {
            id: 1,
            operation: Operation::Const(Literal::Integer(value.into())),
            inputs: vec![],
            outputs: vec![Port {
                ty: integer(value.into(), value.into()),
                ..input.clone()
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        });
        SourceEndpoint::NodeOutput { node: 1, port: 0 }
    } else {
        graph.inputs.push(input.clone());
        SourceEndpoint::GraphInput(0)
    };
    graph.nodes.push(Node {
        id: 2,
        operation: Operation::Truncate { bits, signed },
        inputs: vec![input],
        outputs: graph.outputs.clone(),
        effects: BTreeSet::new(),
        required_capabilities: BTreeSet::new(),
    });
    graph.edges = vec![
        Edge {
            from: source,
            to: TargetEndpoint::NodeInput { node: 2, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    graph
}

// An independent arithmetic oracle: Euclidean remainder, rather than the
// optimizer/backend's masking and sign-extension implementation.
fn expected(value: i64, bits: u16, signed: bool) -> i128 {
    let modulus = 1_i128 << bits;
    let low = i128::from(value).rem_euclid(modulus);
    if signed && low >= modulus / 2 {
        low - modulus
    } else {
        low
    }
}

#[test]
fn all_widths_preserve_runtime_truncation_and_fold_constants_exactly() {
    for bits in 1..=64 {
        for signed in [false, true] {
            let compiled =
                compile_graph(&graph(bits, signed, None), MachineProfile::x86_64_v3()).unwrap();
            assert!(
                compiled.machine_ir.operations.iter().any(|instruction| {
                    matches!(instruction.op, MachineOp::Truncate {
                    bits: actual_bits, signed: actual_signed, ..
                } if actual_bits == bits && actual_signed == signed)
                }),
                "runtime operation lost: bits={bits}, signed={signed}"
            );

            let boundary = 1_i128 << (bits - 1);
            let mut values = vec![i64::MIN, i64::MAX, -1, 0, 1, -257, 257];
            for value in [boundary - 1, boundary, -boundary, -boundary - 1] {
                if let Ok(value) = i64::try_from(value) {
                    values.push(value);
                }
            }
            for value in values {
                let input = graph(bits, signed, Some(value));
                let (folded, _) = optimize_gir(&input).unwrap();
                let result = folded.nodes.iter().find(|node| node.id == 2).unwrap();
                assert_eq!(
                    result.operation,
                    Operation::Const(Literal::Integer(expected(value, bits, signed))),
                    "bits={bits}, signed={signed}, value={value}"
                );
                let compiled = compile_graph(&input, MachineProfile::x86_64_v3()).unwrap();
                assert!(
                    !compiled.machine_ir.operations.iter().any(|instruction| {
                        matches!(instruction.op, MachineOp::Truncate { .. })
                    })
                );
            }
        }
    }
}

// The emitter targets ELF and the System V ABI. Run its actual machine code
// on that platform; the portable test above still covers Windows hosts.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[test]
fn native_execution_matches_bit_contract_for_every_width() {
    use g0::x86_codegen::emit_x86_64_named;
    use std::fmt::Write;
    use std::process::Command;

    let directory = std::env::temp_dir().join(format!(
        "g0-native-truncate-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&directory).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(directory.clone());
    let mut assembly = String::new();
    let mut harness = include_str!("support/native_truncate.c").to_owned();
    let mut checks = String::new();
    for bits in 1..=64 {
        for signed in [false, true] {
            let symbol = format!("g0_truncate_{bits}_{}", u8::from(signed));
            for (suffix, constant) in [("", None), ("_folded", Some(-1))] {
                let compiled =
                    compile_graph(&graph(bits, signed, constant), MachineProfile::x86_64_v3())
                        .unwrap();
                assembly.push_str(
                    &emit_x86_64_named(&compiled.machine_ir, &format!("{symbol}{suffix}"), true)
                        .unwrap(),
                );
            }
            writeln!(
                harness,
                "extern uint64_t {symbol}(int64_t);\nextern uint64_t {symbol}_folded(void);"
            )
            .unwrap();
            writeln!(
                checks,
                "    if (check({symbol}, {symbol}_folded, {bits}, {})) return 1;",
                u8::from(signed)
            )
            .unwrap();
        }
    }
    assembly.push_str(".section .note.GNU-stack,\"\",@progbits\n");
    writeln!(harness, "int main(void) {{\n{checks}    return 0;\n}}").unwrap();
    std::fs::write(directory.join("truncate.s"), assembly).unwrap();
    std::fs::write(directory.join("harness.c"), harness).unwrap();
    let build = Command::new("cc")
        .current_dir(&directory)
        .args([
            "-std=c11",
            "-Wall",
            "-Wextra",
            "-Werror",
            "truncate.s",
            "harness.c",
            "-o",
            "truncate",
        ])
        .output()
        .expect("native truncation tests require a C compiler");
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let run = Command::new(directory.join("truncate")).output().unwrap();
    assert!(
        run.status.success(),
        "native harness failed: {:?}\n{}",
        run.status,
        String::from_utf8_lossy(&run.stderr)
    );
}
