use g0::gir::*;

#[test]
fn contextual_pipeline_keeps_unused_nested_resource_proofs() {
    let program = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&program, compiler_limits()).unwrap();
    for in_schema in [false, true] {
        let document = g0::program_binary::ProgramDocument {
            entry_graph: "main".into(),
            graphs: vec![g0::editor::GraphEditor::new().graph().clone()],
            schemas: vec![g0::data_format::DataSchema {
                name: "Shape".into(),
                version: 1,
                fields: vec![],
            }],
        };
        let mut source = g0::program_binary::encode_program(&document).unwrap();
        if in_schema {
            let fields = source.len() - 4;
            source[fields..].copy_from_slice(&1u32.to_le_bytes());
            source.extend(1u32.to_le_bytes());
            source.extend(6u32.to_le_bytes());
            source.extend(b"unused");
            source.extend(3u32.to_le_bytes());
            source.extend([14, 17, 0]);
            source.push(0);
        } else {
            let ast = executor
                .run_graph(
                    "reader-program-ast",
                    vec![Value::Bytes(source.clone().into())],
                )
                .unwrap();
            let Value::Array(rows) = &ast[0] else {
                panic!("rows")
            };
            let Value::Array(row) = &rows[0] else {
                panic!("row")
            };
            let Value::Integer(start) = row[0] else {
                panic!("start")
            };
            let Value::Integer(inputs) = row[3] else {
                panic!("inputs")
            };
            let length_at = start as usize - 4;
            let old_length =
                u32::from_le_bytes(source[length_at..length_at + 4].try_into().unwrap());
            source[length_at..length_at + 4].copy_from_slice(&(old_length + 9).to_le_bytes());
            let inputs = inputs as usize;
            source[inputs..inputs + 4].copy_from_slice(&1u32.to_le_bytes());
            let mut port = 0u16.to_le_bytes().to_vec();
            port.extend(0u32.to_le_bytes());
            port.extend([14, 17, 0]);
            source.splice(inputs + 4..inputs + 4, port);
        }
        let bytes = Value::Bytes(source.into());
        let rows = executor
            .run_graph("reader-program-ast", vec![bytes.clone()])
            .unwrap()
            .remove(0);
        let schemas = executor
            .run_graph("reader-program-schemas", vec![bytes.clone()])
            .unwrap()
            .remove(0);
        let checked = if in_schema {
            "validator-program-schema-types-context"
        } else {
            "validator-program-port-types-context"
        };
        assert_eq!(
            executor
                .run_graph(checked, vec![bytes.clone(), rows, schemas])
                .unwrap(),
            vec![Value::Bool(false)]
        );
        if !in_schema {
            assert_eq!(
                executor
                    .run_graph("control-domain", vec![bytes.clone()])
                    .unwrap(),
                vec![Value::Bool(false)]
            );
        }
        let result = executor.run_graph("compile-direct", vec![bytes]).unwrap();
        let [Value::Text(text)] = result.as_slice() else {
            panic!("diagnostic")
        };
        assert!(text.starts_with("G0 compiler:"), "{text}");
    }
}
use g0::{bootstrap_compiler::*, execution::Executor, value::Value};
#[test]
fn shared_tables_preserve_each_phase_and_reduce_total_work() {
    for kind in ["valid", "invalid-schema", "select", "unordered"] {
        compare(kind);
    }
}
fn compare(kind: &str) {
    let mut main = g0::editor::GraphEditor::new().graph().clone();
    let mut graphs = vec![main.clone()];
    if matches!(kind, "select" | "unordered") {
        let mut branch = main.clone();
        branch.name = "left".into();
        branch.outputs = main.nodes[0].outputs.clone();
        let mut right = branch.clone();
        right.name = "right".into();
        let mut select = main.nodes[0].clone();
        select.id = 2;
        select.operation = Operation::Select {
            when_true: "left".into(),
            when_false: "right".into(),
        };
        select.inputs = vec![Port {
            id: 0,
            name: "selector".into(),
            ty: SemanticType::Bool,
        }];
        main.nodes[0].operation = Operation::Const(Literal::Bool(true));
        main.nodes[0].outputs[0].ty = SemanticType::Bool;
        main.nodes.push(select);
        main.edges[0].from = SourceEndpoint::NodeOutput { node: 2, port: 0 };
        main.edges.push(Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::NodeInput { node: 2, port: 0 },
        });
        graphs = vec![branch, main, right];
    }
    let document = g0::program_binary::ProgramDocument {
        entry_graph: "main".into(),
        graphs,
        schemas: vec![g0::data_format::DataSchema {
            name: "Shape".into(),
            version: 1,
            fields: vec![],
        }],
    };
    let mut source = g0::program_binary::encode_program(&document).unwrap();
    if kind == "invalid-schema" {
        let version = source.len() - 8;
        source[version..version + 4].fill(0);
    }
    if kind == "unordered" {
        // Reorder raw graph blobs without an encoder canonicalizing them again.
        let mut at = 20usize;
        let mut blobs = Vec::new();
        for _ in 0..3 {
            let size = u32::from_le_bytes(source[at..at + 4].try_into().unwrap()) as usize;
            blobs.push(source[at..at + 4 + size].to_vec());
            at += size + 4;
        }
        let mut raw = source[..20].to_vec();
        for blob in blobs.into_iter().rev() {
            raw.extend(blob);
        }
        raw.extend_from_slice(&source[at..]);
        source = raw;
    }
    let program = compiler_document().validated_contract().unwrap();
    let bytes = Value::Bytes(source.into());
    let mut prepared = Executor::new(&program, compiler_limits()).unwrap();
    let rows = prepared
        .run_graph("reader-program-ast", vec![bytes.clone()])
        .unwrap()
        .remove(0);
    let mut shared_steps = prepared.steps_used();
    let schemas = prepared
        .run_graph("reader-program-schemas", vec![bytes.clone()])
        .unwrap()
        .remove(0);
    shared_steps += prepared.steps_used();
    let mut original = Executor::new(&program, compiler_limits()).unwrap();
    let mut original_steps = 0;
    for name in [
        "control-domain",
        "validator-program-names",
        "validator-program-references",
        "validator-program-callcycles-fast",
        "validator-program-schemas",
        "validator-program-schema-types",
        "validator-program-port-types",
        "control-compile",
    ] {
        let expected = original.run_graph(name, vec![bytes.clone()]).unwrap();
        original_steps += original.steps_used();
        let actual = prepared
            .run_graph(
                &format!("{name}-context"),
                vec![bytes.clone(), rows.clone(), schemas.clone()],
            )
            .unwrap();
        shared_steps += prepared.steps_used();
        assert_eq!(actual, expected, "{name}");
    }
    assert!(
        shared_steps < original_steps,
        "shared {} original {}",
        shared_steps,
        original_steps
    );
    println!("kind={kind} shared={shared_steps} original={original_steps}");
    let result = Executor::new(&program, compiler_limits())
        .unwrap()
        .run_graph("compile-direct", vec![bytes])
        .unwrap();
    let [Value::Text(text)] = result.as_slice() else {
        panic!("compiler text")
    };
    assert_eq!(
        text.starts_with(".text"),
        matches!(kind, "valid" | "select"),
        "{kind}: {text}"
    );
}
