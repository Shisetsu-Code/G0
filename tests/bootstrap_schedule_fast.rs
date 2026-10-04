use g0::{
    bootstrap_compiler::{compiler_document, compiler_limits},
    execution::{Executor, RuntimeError},
    value::Value,
};
fn row(values: &[i128]) -> Value {
    Value::Array(
        values
            .iter()
            .copied()
            .map(Value::Integer)
            .collect::<Vec<_>>()
            .into(),
    )
}
fn rows(values: &[&[i128]]) -> Value {
    Value::Array(values.iter().map(|v| row(v)).collect::<Vec<_>>().into())
}
fn run(name: &str, args: Vec<Value>) -> Vec<Value> {
    let program = compiler_document().validated_contract().unwrap();
    Executor::new(&program, compiler_limits())
        .unwrap()
        .run_graph(name, args)
        .unwrap()
}
#[test]
fn scheduler_fast_requires_strict_ids_and_forward_node_dependencies() {
    let nodes = rows(&[
        &[0, 0, 0, 0, 0, 0, 0, 0],
        &[7, 0, 0, 0, 0, 0, 0, 0],
        &[90, 0, 0, 0, 0, 0, 0, 0],
    ]);
    let forward = rows(&[
        &[1, 0, 4, 0, 7, 9],
        &[1, 7, 9, 0, 90, 5],
        &[0, 0, 99, 0, 7, 4],
        &[1, 90, 5, 1, 0, 42],
    ]);
    assert_eq!(
        run("scheduler-fast-eligible", vec![nodes.clone(), forward]),
        vec![Value::Bool(true)]
    );
    for edges in [rows(&[&[1, 90, 5, 0, 7, 9]]), rows(&[&[1, 7, 5, 0, 7, 9]])] {
        assert_eq!(
            run("scheduler-fast-eligible", vec![nodes.clone(), edges]),
            vec![Value::Bool(false)]
        );
    }
    for nodes in [
        rows(&[&[7, 0, 0, 0, 0, 0, 0, 0], &[7, 0, 0, 0, 0, 0, 0, 0]]),
        rows(&[&[90, 0, 0, 0, 0, 0, 0, 0], &[7, 0, 0, 0, 0, 0, 0, 0]]),
    ] {
        assert_eq!(
            run("scheduler-fast-eligible", vec![nodes, rows(&[])]),
            vec![Value::Bool(false)]
        );
    }
    assert_eq!(
        run("scheduler-fast-eligible", vec![rows(&[]), rows(&[])]),
        vec![Value::Bool(true)]
    );
}
#[test]
fn scheduler_fast_falls_back_for_reversed_dependencies_and_cycles() {
    let nodes = rows(&[
        &[1, 0, 0, 0, 0, 0, 0, 0],
        &[2, 0, 0, 0, 0, 0, 0, 0],
        &[3, 0, 0, 0, 0, 0, 0, 0],
    ]);
    let edges = rows(&[&[1, 2, 0, 0, 1, 0], &[1, 3, 0, 0, 1, 1]]);
    assert_eq!(
        run("scheduler-fast-rows", vec![nodes.clone(), edges.clone()]),
        run("scheduler-order", vec![nodes, edges])
    );
    let nodes = rows(&[&[1, 0, 0, 0, 0, 0, 0, 0], &[2, 0, 0, 0, 0, 0, 0, 0]]);
    let cycle = rows(&[&[1, 2, 0, 0, 1, 0], &[1, 1, 0, 0, 2, 0]]);
    assert_eq!(
        run("scheduler-fast-rows", vec![nodes, cycle]),
        vec![row(&[])]
    );
}
#[test]
fn scheduler_fast_reads_graph_ast_at_nonzero_base() {
    let graph = g0::editor::GraphEditor::new().graph().clone();
    let mut source = vec![0; 17];
    source.extend(g0::graph_binary::encode_graph(&graph).unwrap());
    let args = vec![Value::Bytes(source.into()), Value::Integer(17)];
    assert_eq!(
        run("scheduler-fast", args.clone()),
        run("reader-graph-ast", args)
    );
}
#[test]
fn scheduler_fast_avoids_kahn_cost_for_builder_ordered_graphs() {
    let nodes = Value::Array(
        (1..=40)
            .map(|id| row(&[id, 0, 0, 0, 0, 0, 0, 0]))
            .collect::<Vec<_>>()
            .into(),
    );
    let args = vec![nodes.clone(), rows(&[])];
    let program = compiler_document().validated_contract().unwrap();
    let mut limits = compiler_limits();
    limits.max_steps = 6000;
    assert_eq!(
        Executor::new(&program, limits)
            .unwrap()
            .run_graph("scheduler-fast-rows", args.clone())
            .unwrap(),
        vec![nodes]
    );
    assert_eq!(
        Executor::new(&program, limits)
            .unwrap()
            .run_graph("scheduler-order", args),
        Err(RuntimeError::StepLimit)
    );
}
