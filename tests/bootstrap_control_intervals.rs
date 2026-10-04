use g0::{
    bootstrap_compiler::{compiler_document, compiler_limits},
    execution::{Executor, RuntimeError},
    value::Value,
};
fn row(v: &[i128]) -> Value {
    Value::Array(
        v.iter()
            .map(|n| Value::Integer(*n))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn table(v: &[&[i128]]) -> Value {
    Value::Array(v.iter().map(|v| row(v)).collect::<Vec<_>>().into())
}
#[test]
fn private_intervals_cover_empty_sparse_and_u32_boundary_nodes() {
    let p = compiler_document().validated_contract().unwrap();
    let mut e = Executor::new(&p, compiler_limits()).unwrap();
    let nodes = table(&[
        &[0, 0, 0, 0, 0, 0, 0, 0],
        &[17, 0, 0, 0, 0, 0, 0, 0],
        &[u32::MAX as i128, 0, 0, 0, 0, 0, 0, 0],
    ]);
    let edges = table(&[
        &[0, 0, 0, 0, 17, 90],
        &[0, 0, 0, 0, 17, 3],
        &[0, 0, 0, 0, u32::MAX as i128, 1],
        &[0, 0, 0, 1, 0, 0],
    ]);
    let cached = e
        .run_graph("control-fast-edges-cache", vec![edges])
        .unwrap()
        .remove(0);
    assert_eq!(
        e.run_graph(
            "control-fast-node-intervals",
            vec![nodes.clone(), cached.clone()]
        )
        .unwrap(),
        vec![table(&[
            &[0, 0, 0, 0, 0, 0, 0, 0, 1_i128 << 32, 1, 1],
            &[17, 0, 0, 0, 0, 0, 0, 0, 1_i128 << 32, 1, 3],
            &[u32::MAX as i128, 0, 0, 0, 0, 0, 0, 0, 1_i128 << 32, 3, 4]
        ])]
    );
    assert_eq!(
        e.run_graph("control-fast-node-intervals", vec![table(&[]), cached])
            .unwrap(),
        vec![table(&[])]
    );
    let reversed = table(&[&[17, 0, 0, 0, 0, 0, 0, 0], &[0, 0, 0, 0, 0, 0, 0, 0]]);
    let cache = e
        .run_graph("control-fast-edges-cache", vec![table(&[])])
        .unwrap()
        .remove(0);
    assert_eq!(
        e.run_graph("control-fast-node-intervals", vec![reversed.clone(), cache])
            .unwrap(),
        vec![reversed]
    );
    assert_eq!(
        e.run_graph(
            "control-fast-node-intervals",
            vec![nodes.clone(), table(&[&[2, 1, 0]])]
        )
        .unwrap(),
        vec![nodes]
    );
}
#[test]
fn real_compiler_interval_arguments_keep_exact_assembly_with_fewer_steps() {
    let d = compiler_document();
    let p = d.validated_contract().unwrap();
    let mut e = Executor::new(&p, compiler_limits()).unwrap();
    let graph = d.graphs.iter().max_by_key(|g| g.edges.len()).unwrap();
    let source = Value::Bytes(g0::graph_binary::encode_graph(graph).unwrap().into());
    let ast = e
        .run_graph("reader-graph-ast", vec![source.clone(), Value::Integer(0)])
        .unwrap()
        .remove(0);
    let edges = e
        .run_graph(
            "reader-graph-edges",
            vec![source.clone(), Value::Integer(0)],
        )
        .unwrap()
        .remove(0);
    let edges = e
        .run_graph("control-fast-edges-cache", vec![edges])
        .unwrap()
        .remove(0);
    let slots = e
        .run_graph("control-node-slots", vec![source.clone(), ast.clone()])
        .unwrap()
        .remove(1);
    let slots = e
        .run_graph("control-fast-slots-cache", vec![slots])
        .unwrap()
        .remove(0);
    let intervals = e
        .run_graph(
            "control-fast-node-intervals",
            vec![ast.clone(), edges.clone()],
        )
        .unwrap()
        .remove(0);
    let Value::Array(ast) = ast else { panic!() };
    let Value::Array(intervals) = intervals else {
        panic!()
    };
    let node = ast.last().unwrap().clone();
    let enhanced = intervals.last().unwrap().clone();
    let args = vec![
        source,
        node,
        slots,
        edges,
        Value::Integer((8 + 4 + graph.name.len() + 1) as i128),
    ];
    let expected = e.run_graph("control-fast-arguments", args.clone()).unwrap();
    let mut fast = args.clone();
    fast[1] = enhanced.clone();
    assert_eq!(
        e.run_graph("control-fast-arguments", fast.clone()).unwrap(),
        expected
    );
    let mut limits = compiler_limits();
    limits.max_steps = 1_500;
    assert_eq!(
        Executor::new(&p, limits)
            .unwrap()
            .run_graph("control-fast-arguments", fast)
            .unwrap(),
        expected
    );
    assert_eq!(
        Executor::new(&p, limits)
            .unwrap()
            .run_graph("control-fast-arguments", args.clone()),
        Err(RuntimeError::StepLimit)
    );
    let Value::Array(fields) = enhanced else {
        panic!()
    };
    for bad in [
        row(&fields
            .iter()
            .take(8)
            .map(|v| match v {
                Value::Integer(n) => *n,
                _ => panic!(),
            })
            .chain([0, 0, 0])
            .collect::<Vec<_>>()),
        row(&fields
            .iter()
            .take(8)
            .map(|v| match v {
                Value::Integer(n) => *n,
                _ => panic!(),
            })
            .chain([1_i128 << 32])
            .collect::<Vec<_>>()),
        row(&fields
            .iter()
            .take(8)
            .map(|v| match v {
                Value::Integer(n) => *n,
                _ => panic!(),
            })
            .chain([1_i128 << 32, 0, 999999])
            .collect::<Vec<_>>()),
    ] {
        let mut raw = args.clone();
        raw[1] = bad;
        assert_eq!(
            e.run_graph("control-fast-arguments", raw).unwrap(),
            expected
        );
    }
}
