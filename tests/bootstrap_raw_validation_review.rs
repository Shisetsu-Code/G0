use g0::{
    bootstrap_compiler::{compiler_document, compiler_limits},
    execution::Executor,
    value::Value,
};

fn blob(bytes: &mut Vec<u8>, payload: &[u8]) {
    bytes.extend((payload.len() as u32).to_le_bytes());
    bytes.extend(payload);
}
fn ports(types: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = (types.len() as u32).to_le_bytes().to_vec();
    for (i, ty) in types.iter().enumerate() {
        bytes.extend((i as u16).to_le_bytes());
        blob(&mut bytes, format!("p{i}").as_bytes());
        bytes.extend(ty);
    }
    bytes
}
fn node(id: u32, operation: &[u8], inputs: &[Vec<u8>], outputs: &[Vec<u8>]) -> Vec<u8> {
    let mut bytes = id.to_le_bytes().to_vec();
    bytes.extend(operation);
    bytes.extend(ports(inputs));
    bytes.extend(ports(outputs));
    bytes.extend([0; 8]);
    bytes
}
fn graph(
    name: &str,
    inputs: &[Vec<u8>],
    outputs: &[Vec<u8>],
    nodes: &[Vec<u8>],
    edges: &[Vec<u8>],
) -> Vec<u8> {
    let mut bytes = b"G0G\0".to_vec();
    bytes.extend(0u16.to_le_bytes());
    bytes.extend(10u16.to_le_bytes());
    blob(&mut bytes, name.as_bytes());
    bytes.push(0);
    bytes.extend(ports(inputs));
    bytes.extend(ports(outputs));
    bytes.extend((nodes.len() as u32).to_le_bytes());
    for n in nodes {
        bytes.extend(n);
    }
    bytes.extend((edges.len() as u32).to_le_bytes());
    for e in edges {
        bytes.extend(e);
    }
    bytes
}
fn edge(from: u32, source_port: u16, target: Option<u32>, target_port: u16) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend(from.to_le_bytes());
    bytes.extend(source_port.to_le_bytes());
    if let Some(target) = target {
        bytes.push(0);
        bytes.extend(target.to_le_bytes());
    } else {
        bytes.push(1);
    }
    bytes.extend(target_port.to_le_bytes());
    bytes
}
fn program(entry: &str, graphs: &[Vec<u8>], schemas: &[u8], schema_count: u32) -> Vec<u8> {
    let mut bytes = b"G0P\0".to_vec();
    bytes.extend(0u16.to_le_bytes());
    bytes.extend(3u16.to_le_bytes());
    blob(&mut bytes, entry.as_bytes());
    bytes.extend((graphs.len() as u32).to_le_bytes());
    for g in graphs {
        blob(&mut bytes, g);
    }
    bytes.extend(schema_count.to_le_bytes());
    bytes.extend(schemas);
    bytes
}
fn accepted(executor: &mut Executor<'_>, bytes: &[u8]) -> bool {
    match executor.run_graph("compile-direct", vec![Value::Bytes(bytes.into())]) {
        Ok(values) => matches!(values.as_slice(), [Value::Text(text)] if text.starts_with(".text")),
        Err(_) => false,
    }
}
fn boolean_graph(edges: &[Vec<u8>]) -> Vec<u8> {
    graph(
        "main",
        &[],
        &[vec![0]],
        &[
            node(1, &[0, 0, 1], &[], &[vec![0]]),
            node(2, &[22], &[vec![0], vec![0]], &[vec![0]]),
        ],
        edges,
    )
}

#[test]
fn raw_node_dependency_cycles_are_rejected_before_emission() {
    let compiler = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&compiler, compiler_limits()).unwrap();
    let baseline = program(
        "main",
        &[boolean_graph(&[
            edge(1, 0, Some(2), 0),
            edge(1, 0, Some(2), 1),
            edge(2, 0, None, 0),
        ])],
        &[],
        0,
    );
    assert!(g0::program_binary::decode_program(&baseline).is_ok());
    assert!(accepted(&mut executor, &baseline));
    let cyclic = program(
        "main",
        &[boolean_graph(&[
            edge(2, 0, Some(2), 0),
            edge(1, 0, Some(2), 1),
            edge(2, 0, None, 0),
        ])],
        &[],
        0,
    );
    assert!(g0::program_binary::decode_program(&cyclic).is_err());
    assert!(
        !accepted(&mut executor, &cyclic),
        "raw self-dependency emitted native assembly"
    );
}

fn graph_input(port: u16, node: u32, input: u16) -> Vec<u8> {
    let mut bytes = vec![0];
    bytes.extend(port.to_le_bytes());
    bytes.push(0);
    bytes.extend(node.to_le_bytes());
    bytes.extend(input.to_le_bytes());
    bytes
}
fn named_type(tag: u8, name: &str) -> Vec<u8> {
    let mut bytes = vec![tag];
    blob(&mut bytes, name.as_bytes());
    bytes
}
fn schema(name: &str, version: u32, fields: &[(u32, &str, Vec<u8>, u8)]) -> Vec<u8> {
    let mut bytes = vec![];
    blob(&mut bytes, name.as_bytes());
    bytes.extend(version.to_le_bytes());
    bytes.extend((fields.len() as u32).to_le_bytes());
    for (tag, name, ty, required) in fields {
        bytes.extend(tag.to_le_bytes());
        blob(&mut bytes, name.as_bytes());
        blob(&mut bytes, ty);
        bytes.push(*required);
    }
    bytes
}
fn bool_constant(name: &str, inputs: &[Vec<u8>]) -> Vec<u8> {
    graph(
        name,
        inputs,
        &[vec![0]],
        &[node(1, &[0, 0, 1], &[], &[vec![0]])],
        &[edge(1, 0, None, 0)],
    )
}
fn check(executor: &mut Executor<'_>, label: &str, bytes: Vec<u8>, expected: bool) {
    let host = g0::program_binary::decode_program(&bytes).and_then(|d| d.validated_contract());
    assert_eq!(
        host.is_ok(),
        expected,
        "Rust validity for {label}: {host:?}"
    );
    let output = executor.run_graph("compile-direct", vec![Value::Bytes(bytes.into())]);
    let g0_valid = matches!(&output, Ok(values) if matches!(values.as_slice(), [Value::Text(text)] if text.starts_with(".text")));
    assert_eq!(
        g0_valid, expected,
        "G0 compile-direct validity for {label}: {output:?}"
    );
}

#[test]
fn raw_wiring_identifiers_arity_and_container_boundaries_match_rust() {
    let compiler = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&compiler, compiler_limits()).unwrap();
    let edges = vec![
        edge(1, 0, Some(2), 0),
        edge(1, 0, Some(2), 1),
        edge(2, 0, None, 0),
    ];
    for i in 0..3 {
        let mut missing = edges.clone();
        missing.remove(i);
        check(
            &mut executor,
            "missing target",
            program("main", &[boolean_graph(&missing)], &[], 0),
            false,
        );
        for j in 0..3 {
            if i != j {
                let mut duplicate = edges.clone();
                duplicate[i] = duplicate[j].clone();
                check(
                    &mut executor,
                    "duplicate target",
                    program("main", &[boolean_graph(&duplicate)], &[], 0),
                    false,
                );
            }
        }
    }
    for replacement in [
        edge(99, 0, Some(2), 0),
        edge(1, 99, Some(2), 0),
        edge(1, 0, Some(99), 0),
        edge(1, 0, Some(2), 99),
        edge(1, 0, None, 99),
    ] {
        let mut invalid = edges.clone();
        invalid[0] = replacement;
        check(
            &mut executor,
            "missing endpoint",
            program("main", &[boolean_graph(&invalid)], &[], 0),
            false,
        );
    }
    for (id, operation, inputs, outputs) in [
        (1, vec![22], vec![vec![0], vec![0]], vec![vec![0]]),
        (2, vec![22], vec![vec![0]], vec![vec![0]]),
        (2, vec![22], vec![vec![0], vec![0]], vec![vec![0], vec![0]]),
        (2, vec![75], vec![vec![0], vec![0]], vec![vec![0]]),
        (2, vec![22], vec![vec![7], vec![0]], vec![vec![0]]),
    ] {
        let raw = graph(
            "main",
            &[],
            &[vec![0]],
            &[
                node(1, &[0, 0, 1], &[], &[vec![0]]),
                node(id, &operation, &inputs, &outputs),
            ],
            &edges,
        );
        check(
            &mut executor,
            "node ID/opcode/shape",
            program("main", &[raw], &[], 0),
            false,
        );
    }
    let baseline = program("main", &[boolean_graph(&edges)], &[], 0);
    for len in [0, 1, 7, 8, baseline.len() - 1] {
        check(
            &mut executor,
            "truncated container",
            baseline[..len].to_vec(),
            false,
        );
    }
    let mut trailing = baseline.clone();
    trailing.push(0);
    check(&mut executor, "trailing bytes", trailing, false);
    check(
        &mut executor,
        "missing entry",
        program("gone", &[boolean_graph(&edges)], &[], 0),
        false,
    );
    check(
        &mut executor,
        "duplicate graph",
        program(
            "main",
            &[boolean_graph(&edges), boolean_graph(&edges)],
            &[],
            0,
        ),
        false,
    );
}

#[test]
fn raw_schema_registry_and_named_closure_match_rust() {
    let compiler = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&compiler, compiler_limits()).unwrap();
    let constant = bool_constant("main", &[]);
    let valid = schema("Shape", 1, &[(1, "tag", vec![0], 0)]);
    check(
        &mut executor,
        "valid unused registry",
        program("main", std::slice::from_ref(&constant), &valid, 1),
        true,
    );
    for invalid in [
        schema("Shape", 0, &[(1, "tag", vec![0], 0)]),
        schema("Shape", 1, &[(0, "tag", vec![0], 0)]),
        schema("Shape", 1, &[(1, "tag", vec![0], 2)]),
        schema(
            "Shape",
            1,
            &[(1, "tag", vec![0], 0), (1, "other", vec![0], 0)],
        ),
        schema(
            "Shape",
            1,
            &[(1, "tag", vec![0], 0), (2, "tag", vec![0], 0)],
        ),
        schema("Shape", 1, &[(1, "tag", named_type(12, "Absent"), 0)]),
    ] {
        check(
            &mut executor,
            "invalid schema registry",
            program("main", std::slice::from_ref(&constant), &invalid, 1),
            false,
        );
    }
    let mut duplicate = valid.clone();
    duplicate.extend(&valid);
    check(
        &mut executor,
        "duplicate schema",
        program("main", &[constant], &duplicate, 2),
        false,
    );
    for tag in [12, 13] {
        check(
            &mut executor,
            "known named unused graph input",
            program(
                "main",
                &[bool_constant("main", &[named_type(tag, "Shape")])],
                &valid,
                1,
            ),
            true,
        );
        check(
            &mut executor,
            "unknown named unused graph input",
            program(
                "main",
                &[bool_constant("main", &[named_type(tag, "Absent")])],
                &valid,
                1,
            ),
            false,
        );
    }
}

#[test]
fn raw_callcycles_and_match_defaults_match_rust() {
    let compiler = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&compiler, compiler_limits()).unwrap();
    let call = |name: &str, target: &str| {
        let mut op = vec![7];
        blob(&mut op, target.as_bytes());
        graph(
            name,
            &[],
            &[vec![0]],
            &[node(1, &op, &[], &[vec![0]])],
            &[edge(1, 0, None, 0)],
        )
    };
    check(
        &mut executor,
        "valid call DAG",
        program(
            "main",
            &[bool_constant("body", &[]), call("main", "body")],
            &[],
            0,
        ),
        true,
    );
    check(
        &mut executor,
        "self call",
        program("main", &[call("main", "main")], &[], 0),
        false,
    );
    check(
        &mut executor,
        "mutual calls",
        program(
            "main",
            &[call("body", "main"), call("main", "body")],
            &[],
            0,
        ),
        false,
    );
    check(
        &mut executor,
        "missing callee",
        program("main", &[call("main", "gone")], &[], 0),
        false,
    );
    let schema = schema("Shape", 1, &[(1, "tag", vec![0], 0)]);
    for (tag, default, expected) in [
        ("tag", "body", true),
        ("unknown", "body", true),
        ("", "body", false),
        ("tag", "", false),
        ("tag", "gone", false),
    ] {
        let mut op = vec![5];
        op.extend(1u32.to_le_bytes());
        blob(&mut op, tag.as_bytes());
        blob(&mut op, b"body");
        blob(&mut op, default.as_bytes());
        let main = graph(
            "main",
            &[named_type(13, "Shape")],
            &[vec![0]],
            &[node(1, &op, &[named_type(13, "Shape")], &[vec![0]])],
            &[graph_input(0, 1, 0), edge(1, 0, None, 0)],
        );
        check(
            &mut executor,
            "Match tag/default",
            program("main", &[bool_constant("body", &[]), main], &schema, 1),
            expected,
        );
    }
}

fn integer(min: i128, max: i128) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend(min.to_le_bytes());
    bytes.extend(max.to_le_bytes());
    bytes
}
#[test]
fn raw_integer_depth_width_precision_and_old_opcode_gates_match_rust() {
    let compiler = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&compiler, compiler_limits()).unwrap();
    for (depth, expected) in [(127, true), (128, false), (129, false)] {
        let mut ty = vec![14; depth];
        ty.push(0);
        check(
            &mut executor,
            "type depth",
            program("main", &[bool_constant("main", &[ty])], &[], 0),
            expected,
        );
    }
    fn wide(depth: usize) -> Vec<u8> {
        if depth == 0 {
            vec![0]
        } else {
            let child = wide(depth - 1);
            let mut out = vec![15];
            out.extend(&child);
            out.extend(child);
            out
        }
    }
    check(
        &mut executor,
        "wide Result tree",
        program("main", &[bool_constant("main", &[wide(8)])], &[], 0),
        true,
    );
    for (min, max, expected) in [
        (i128::MIN, i128::MAX, true),
        (i128::MIN, i128::MIN, true),
        (i128::MAX, i128::MAX, true),
        (1, 0, false),
        (i128::MAX, i128::MIN, false),
    ] {
        check(
            &mut executor,
            "signed integer interval",
            program(
                "main",
                &[bool_constant("main", &[integer(min, max)])],
                &[],
                0,
            ),
            expected,
        );
    }
    for ty in [
        vec![4, 0, 0, 0, 0, 0, 0, 0, 0],
        vec![6, 0, 0, 0, 0],
        vec![6, 1, 0, 0, 0],
    ] {
        check(
            &mut executor,
            "invalid numeric precision",
            program("main", &[bool_constant("main", &[ty])], &[], 0),
            false,
        );
    }
    let full = integer(i128::MIN, i128::MAX);
    let codec = graph(
        "main",
        &[vec![8]],
        std::slice::from_ref(&full),
        &[node(1, &[74], &[vec![8]], std::slice::from_ref(&full))],
        &[graph_input(0, 1, 0), edge(1, 0, None, 0)],
    );
    check(
        &mut executor,
        "GIR10 codec",
        program("main", std::slice::from_ref(&codec), &[], 0),
        true,
    );
    for minor in 1u16..10 {
        let mut legacy = codec.clone();
        legacy[6..8].copy_from_slice(&minor.to_le_bytes());
        check(
            &mut executor,
            "old codec version",
            program("main", &[legacy], &[], 0),
            false,
        );
    }
}

#[test]
fn raw_record_variant_payload_contracts_match_rust() {
    let compiler = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&compiler, compiler_limits()).unwrap();
    let required = schema("Shape", 1, &[(1, "tag", vec![0], 0)]);
    let optional = schema("Shape", 1, &[(1, "tag", vec![0], 1)]);
    for (names, input_ty, registry, expected) in [
        (vec!["tag"], vec![0], required.clone(), true),
        (vec![], vec![0], required.clone(), false),
        (vec![], vec![0], optional.clone(), true),
        (vec!["gone"], vec![0], required.clone(), false),
        (vec!["tag", "tag"], vec![0], required.clone(), false),
        (vec!["tag"], vec![8], required.clone(), false),
    ] {
        let mut op = vec![38];
        blob(&mut op, b"Shape");
        op.extend((names.len() as u32).to_le_bytes());
        for name in &names {
            blob(&mut op, name.as_bytes());
        }
        let ins = vec![input_ty; names.len()];
        let outs = vec![named_type(12, "Shape")];
        let mut edges = (0..ins.len())
            .map(|i| graph_input(i as u16, 1, i as u16))
            .collect::<Vec<_>>();
        edges.push(edge(1, 0, None, 0));
        let main = graph("main", &ins, &outs, &[node(1, &op, &ins, &outs)], &edges);
        check(
            &mut executor,
            "MakeRecord fields",
            program("main", &[main], &registry, 1),
            expected,
        );
    }
    for (tag, input_ty, expected) in [
        ("tag", vec![0], true),
        ("gone", vec![0], false),
        ("tag", vec![8], false),
    ] {
        let mut op = vec![40];
        blob(&mut op, b"Shape");
        blob(&mut op, tag.as_bytes());
        let main = graph(
            "main",
            std::slice::from_ref(&input_ty),
            &[named_type(13, "Shape")],
            &[node(
                1,
                &op,
                std::slice::from_ref(&input_ty),
                &[named_type(13, "Shape")],
            )],
            &[graph_input(0, 1, 0), edge(1, 0, None, 0)],
        );
        check(
            &mut executor,
            "MakeVariant payload",
            program("main", &[main], &required, 1),
            expected,
        );
    }
    for (opcode, tag, output, registry, expected) in [
        (39, "tag", vec![0], required.clone(), true),
        (39, "tag", vec![14, 0], optional.clone(), true),
        (39, "tag", vec![0], optional.clone(), false),
        (39, "gone", vec![0], required.clone(), false),
        (41, "tag", vec![14, 0], required.clone(), true),
        (41, "gone", vec![14, 0], required.clone(), false),
        (41, "tag", vec![0], required.clone(), false),
    ] {
        let mut op = vec![opcode];
        blob(&mut op, tag.as_bytes());
        let input_ty = named_type(if opcode == 39 { 12 } else { 13 }, "Shape");
        let main = graph(
            "main",
            std::slice::from_ref(&input_ty),
            std::slice::from_ref(&output),
            &[node(
                1,
                &op,
                std::slice::from_ref(&input_ty),
                std::slice::from_ref(&output),
            )],
            &[graph_input(0, 1, 0), edge(1, 0, None, 0)],
        );
        check(
            &mut executor,
            "Field/VariantPayload output",
            program("main", &[main], &registry, 1),
            expected,
        );
    }
}

#[test]
fn raw_select_loop_map_contracts_match_rust() {
    let compiler = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&compiler, compiler_limits()).unwrap();
    let identity = |name: &str, ty: Vec<u8>| {
        let mut e = vec![0];
        e.extend(0u16.to_le_bytes());
        e.push(1);
        e.extend(0u16.to_le_bytes());
        graph(
            name,
            std::slice::from_ref(&ty),
            std::slice::from_ref(&ty),
            &[],
            &[e],
        )
    };
    for (body_ty, expected) in [(vec![0], true), (vec![8], false)] {
        let mut op = vec![4];
        blob(&mut op, b"body");
        blob(&mut op, b"body");
        let main = graph(
            "main",
            &[],
            &[vec![0]],
            &[
                node(1, &[0, 0, 1], &[], &[vec![0]]),
                node(2, &op, &[vec![0], vec![0]], &[vec![0]]),
            ],
            &[
                edge(1, 0, Some(2), 0),
                edge(1, 0, Some(2), 1),
                edge(2, 0, None, 0),
            ],
        );
        let mut body = identity("body", body_ty);
        let name = body.windows(2).position(|w| w == b"p0").unwrap();
        body[name..name + 2].copy_from_slice(b"p1");
        check(
            &mut executor,
            "Select exact contract",
            program("main", &[body, main], &[], 0),
            expected,
        );
    }
    for (bound, condition_output, expected) in [
        (1u64, vec![0], true),
        (0, vec![0], false),
        (1, vec![8], false),
    ] {
        let mut op = vec![6];
        blob(&mut op, b"condition");
        blob(&mut op, b"body");
        op.extend(bound.to_le_bytes());
        let condition_op = if condition_output == vec![0] {
            vec![0, 0, 1]
        } else {
            vec![0, 3, 0, 0, 0, 0]
        };
        let condition = graph(
            "condition",
            &[vec![0]],
            std::slice::from_ref(&condition_output),
            &[node(
                1,
                &condition_op,
                &[],
                std::slice::from_ref(&condition_output),
            )],
            &[edge(1, 0, None, 0)],
        );
        let main = graph(
            "main",
            &[vec![0]],
            &[vec![0]],
            &[node(1, &op, &[vec![0]], &[vec![0]])],
            &[graph_input(0, 1, 0), edge(1, 0, None, 0)],
        );
        check(
            &mut executor,
            "Loop condition/bound",
            program(
                "main",
                &[identity("body", vec![0]), condition, main],
                &[],
                0,
            ),
            expected,
        );
    }
    for (max, expected) in [(255, true), (254, false), (256, false)] {
        let mut op = vec![47];
        blob(&mut op, b"body");
        let main = graph(
            "main",
            &[vec![8]],
            &[vec![10, 0]],
            &[node(1, &op, &[vec![8]], &[vec![10, 0]])],
            &[graph_input(0, 1, 0), edge(1, 0, None, 0)],
        );
        check(
            &mut executor,
            "Map byte element exactness",
            program(
                "main",
                &[bool_constant("body", &[integer(0, max)]), main],
                &[],
                0,
            ),
            expected,
        );
    }
}
