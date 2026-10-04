use g0::{
    bootstrap_compiler::{compiler_document, compiler_limits},
    execution::{Executor, RuntimeError},
    gir::SemanticType,
    value::Value,
};
#[path = "support/program_fixture.rs"]
mod fixture;
fn row(ns: &[i128]) -> Value {
    Value::Array(
        ns.iter()
            .copied()
            .map(Value::Integer)
            .collect::<Vec<_>>()
            .into(),
    )
}
fn rows(ns: &[&[i128]]) -> Value {
    Value::Array(ns.iter().map(|n| row(n)).collect::<Vec<_>>().into())
}
fn run(name: &str, args: Vec<Value>) -> Vec<Value> {
    let d = compiler_document();
    let p = d.validated_contract().unwrap();
    Executor::new(&p, compiler_limits())
        .unwrap()
        .run_graph(name, args)
        .unwrap()
}
#[test]
fn control_fast_slot_cache_proves_sorting_and_matches_general_lookup() {
    for (table, ordered) in [
        (rows(&[&[0, 7, 9], &[5, 4, 6], &[80, 99, 3]]), true),
        (rows(&[&[80, 99, 3], &[0, 7, 9], &[5, 4, 6]]), false),
    ] {
        let cached = run("control-fast-slots-cache", vec![table.clone()])[0].clone();
        let Value::Array(items) = &cached else {
            panic!()
        };
        assert_eq!(items[0], row(&[1_i128 << 32, i128::from(ordered), 3]));
        for (node, port) in [(0, 7), (5, 4), (80, 99), (99, 1)] {
            let args = vec![table.clone(), Value::Integer(node), Value::Integer(port)];
            let expected = run("control-slot-index", args);
            assert_eq!(
                run(
                    "control-fast-slot-index",
                    vec![cached.clone(), Value::Integer(node), Value::Integer(port)]
                ),
                expected
            );
        }
    }
}
#[test]
fn control_fast_edge_cache_sorts_disordered_targets_and_complete_header() {
    for (edges, _sorted) in [
        (
            rows(&[
                &[0, 0, 2, 0, 0, 4],
                &[1, 5, 4, 0, 7, 9],
                &[1, 7, 9, 1, 0, 3],
            ]),
            true,
        ),
        (rows(&[&[1, 7, 9, 1, 0, 3], &[0, 0, 2, 0, 0, 4]]), false),
    ] {
        let cache = run("control-fast-edges-cache", vec![edges])[0].clone();
        let Value::Array(items) = cache else { panic!() };
        let Value::Array(header) = &items[0] else {
            panic!()
        };
        assert_eq!(header.len(), 6);
        assert_eq!(header[0], Value::Integer(2));
        assert_eq!(header[1], Value::Integer(1));
        assert_eq!(header[3], Value::Integer(2));
    }
}

#[test]
fn own_compiler_source_ordered_edges_prepare_a_stable_target_cache() {
    let document = compiler_document();
    let program = document.validated_contract().unwrap();
    let graph = document
        .graphs
        .iter()
        .find(|g| g.name == "reader-u32-definition")
        .unwrap_or_else(|| {
            document
                .graphs
                .iter()
                .max_by_key(|g| g.edges.len())
                .unwrap()
        });
    let bytes = g0::graph_binary::encode_graph(graph).unwrap();
    let mut executor = Executor::new(&program, compiler_limits()).unwrap();
    let source = Value::Bytes(bytes.into());
    let edges = executor
        .run_graph(
            "reader-graph-edges",
            vec![source.clone(), Value::Integer(0)],
        )
        .unwrap()
        .remove(0);
    assert_eq!(
        executor
            .run_graph("control-fast-edges-sorted", vec![edges.clone()])
            .unwrap(),
        vec![Value::Bool(false)],
        "fixture must exercise canonical SOURCE ordering"
    );
    let mut preparation_limits = compiler_limits();
    preparation_limits.max_steps = 1_000_000;
    let cache = Executor::new(&program, preparation_limits)
        .unwrap()
        .run_graph("control-fast-edges-cache", vec![edges.clone()])
        .unwrap()
        .remove(0);
    let Value::Array(original) = &edges else {
        panic!()
    };
    let Value::Array(cached) = &cache else {
        panic!()
    };
    assert_eq!(
        cached[0],
        row(&[2, 1, original.len() as i128, 2, 1_i128 << 32, 1_i128 << 32])
    );
    let mut expected = original.to_vec();
    expected.sort_by_key(|v| {
        let Value::Array(r) = v else { panic!() };
        match (&r[3], &r[4]) {
            (Value::Integer(kind), Value::Integer(node)) => (*kind, *node),
            _ => panic!(),
        }
    });
    assert_eq!(&cached[1..], expected.as_slice());
    let nodes = executor
        .run_graph("reader-graph-ast", vec![source.clone(), Value::Integer(0)])
        .unwrap()
        .remove(0);
    let slots = executor
        .run_graph("control-node-slots", vec![source.clone(), nodes.clone()])
        .unwrap()
        .remove(1);
    let cached_slots = executor
        .run_graph("control-fast-slots-cache", vec![slots.clone()])
        .unwrap()
        .remove(0);
    let Value::Array(nodes) = nodes else { panic!() };
    let node = nodes.last().unwrap().clone();
    let inputs_offset = 8 + 4 + graph.name.len() + 1;
    let general = vec![
        source.clone(),
        node.clone(),
        slots,
        edges,
        Value::Integer(inputs_offset as i128),
    ];
    let expected = executor
        .run_graph("control-arguments", general.clone())
        .unwrap();
    let fast = vec![
        source,
        node,
        cached_slots,
        cache,
        Value::Integer(inputs_offset as i128),
    ];
    let mut limits = compiler_limits();
    limits.max_steps = 12_000;
    assert_eq!(
        Executor::new(&program, limits)
            .unwrap()
            .run_graph("control-fast-arguments", fast)
            .unwrap(),
        expected
    );
    assert_eq!(
        Executor::new(&program, limits)
            .unwrap()
            .run_graph("control-arguments", general),
        Err(RuntimeError::StepLimit)
    );
}

#[test]
fn stable_merge_handles_empty_singleton_equal_targets_and_budget_exhaustion() {
    let p = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&p, compiler_limits()).unwrap();
    for table in [
        rows(&[]),
        rows(&[&[1, 7, 8, 0, 4, 9]]),
        rows(&[&[1, 7, 8, 0, 4, 9], &[0, 0, 2, 0, 4, 1]]),
    ] {
        assert_eq!(
            executor
                .run_graph("control-fast-edges-merge-sort", vec![table.clone()])
                .unwrap(),
            vec![table]
        );
    }
    let table = Value::Array(
        (0..256)
            .rev()
            .map(|n| row(&[0, 0, 0, 0, n, 0]))
            .collect::<Vec<_>>()
            .into(),
    );
    let mut limits = compiler_limits();
    limits.max_steps = 100;
    assert_eq!(
        Executor::new(&p, limits)
            .unwrap()
            .run_graph("control-fast-edges-merge-sort", vec![table]),
        Err(RuntimeError::StepLimit)
    );
}
fn ports(bytes: &mut Vec<u8>, ids: &[u16]) -> usize {
    let at = bytes.len();
    bytes.extend_from_slice(&(ids.len() as u32).to_le_bytes());
    for id in ids {
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend(g0::graph_binary::encode_semantic_type(&SemanticType::Bool).unwrap());
    }
    at
}
#[test]
fn control_fast_arguments_preserve_exact_assembly_for_sparse_and_unordered_edges() {
    let mut bytes = vec![];
    let inputs = ports(&mut bytes, &[2, 7]);
    let node_inputs = ports(&mut bytes, &[4, 9]);
    let node = row(&[7, 0, 0, node_inputs as i128, 0, 0, 0, 0]);
    let slots = rows(&[&[5, 4, 6]]);
    for edges in [
        rows(&[
            &[0, 0, 2, 0, 7, 4],
            &[1, 5, 4, 0, 7, 9],
            &[0, 0, 7, 0, 80, 1],
        ]),
        rows(&[
            &[0, 0, 7, 0, 80, 1],
            &[1, 5, 4, 0, 7, 9],
            &[0, 0, 2, 0, 7, 4],
        ]),
    ] {
        let expected = run(
            "control-arguments",
            vec![
                Value::Bytes(bytes.clone().into()),
                node.clone(),
                slots.clone(),
                edges.clone(),
                Value::Integer(inputs as i128),
            ],
        );
        let cached = run("control-fast-edges-cache", vec![edges])[0].clone();
        let slots = run("control-fast-slots-cache", vec![slots.clone()])[0].clone();
        assert_eq!(
            run(
                "control-fast-arguments",
                vec![
                    Value::Bytes(bytes.clone().into()),
                    node.clone(),
                    slots,
                    cached,
                    Value::Integer(inputs as i128)
                ]
            ),
            expected
        );
    }
}
#[test]
fn control_fast_slot_lookup_stays_bounded_after_cache_preparation() {
    let table = Value::Array(
        (0..1024)
            .map(|n| row(&[n * 11, 9, n]))
            .collect::<Vec<_>>()
            .into(),
    );
    let cached = run("control-fast-slots-cache", vec![table.clone()])[0].clone();
    let d = compiler_document();
    let p = d.validated_contract().unwrap();
    let mut limits = compiler_limits();
    limits.max_steps = 3000;
    let args = vec![cached, Value::Integer(1023 * 11), Value::Integer(9)];
    assert_eq!(
        Executor::new(&p, limits)
            .unwrap()
            .run_graph("control-fast-slot-index", args)
            .unwrap(),
        vec![Value::Integer(1023)]
    );
    assert_eq!(
        Executor::new(&p, limits).unwrap().run_graph(
            "control-slot-index",
            vec![table, Value::Integer(1023 * 11), Value::Integer(9)]
        ),
        Err(RuntimeError::StepLimit)
    );
}

#[test]
fn control_fast_arguments_visit_only_the_selected_edge_interval() {
    let mut bytes = vec![];
    let inputs = ports(&mut bytes, &[2]);
    let node_inputs = ports(&mut bytes, &[4]);
    let edges = Value::Array(
        (0..1024)
            .map(|n| row(&[0, 0, 2, 0, n * 11, 4]))
            .collect::<Vec<_>>()
            .into(),
    );
    let cached = run("control-fast-edges-cache", vec![edges.clone()])[0].clone();
    let node = row(&[1023 * 11, 0, 0, node_inputs as i128, 0, 0, 0, 0]);
    let slots = rows(&[]);
    let original_args = vec![
        Value::Bytes(bytes.into()),
        node,
        slots,
        edges,
        Value::Integer(inputs as i128),
    ];
    let expected = run("control-arguments", original_args.clone());
    let mut fast_args = original_args.clone();
    fast_args[3] = cached;
    let d = compiler_document();
    let p = d.validated_contract().unwrap();
    let mut limits = compiler_limits();
    limits.max_steps = 6000;
    assert_eq!(
        Executor::new(&p, limits)
            .unwrap()
            .run_graph("control-fast-arguments", fast_args)
            .unwrap(),
        expected
    );
    assert_eq!(
        Executor::new(&p, limits)
            .unwrap()
            .run_graph("control-arguments", original_args),
        Err(RuntimeError::StepLimit)
    );
}

#[test]
fn control_fast_whole_emission_matches_general_scanner_definitions() {
    use g0::gir::Operation;
    let fast = compiler_document();
    // Build a reference compiler from the same graph DEFINITIONS, redirecting
    // emission lookups to the general scanners. Input GIR bytes are untouched.
    let mut reference = fast.clone();
    for graph in &mut reference.graphs {
        for node in &mut graph.nodes {
            if let Operation::Subgraph(target) = &mut node.operation {
                let old = match target.as_str() {
                    "control-fast-slot-index" => Some("control-slot-index"),
                    "control-fast-arguments" => Some("control-arguments"),
                    "validator-node-index-fast" => Some("emitter-node-index"),
                    _ => None,
                };
                if let Some(old) = old {
                    *target = old.into();
                }
            }
        }
    }
    let fast = fast.validated_contract().unwrap();
    let reference = reference.validated_contract().unwrap();
    for document in [
        fixture::call_program(),
        fixture::select_program(),
        fixture::loop_program(),
    ] {
        let source = g0::program_binary::encode_program(&document).unwrap();
        let args = vec![Value::Bytes(source.into())];
        let expected = Executor::new(&reference, compiler_limits())
            .unwrap()
            .run_graph("control-compile", args.clone())
            .unwrap();
        assert_eq!(
            Executor::new(&fast, compiler_limits())
                .unwrap()
                .run_graph("control-compile", args)
                .unwrap(),
            expected
        );
    }
}
