use g0::{bootstrap_compiler::*, execution::Executor, value::Value};
fn integers(xs: &[i128]) -> Value {
    Value::Array(
        xs.iter()
            .copied()
            .map(Value::Integer)
            .collect::<Vec<_>>()
            .into(),
    )
}
fn collection(xs: &[&[i128]]) -> Value {
    Value::Array(xs.iter().map(|v| integers(v)).collect::<Vec<_>>().into())
}
fn schedule(nodes: Value, edges: Value) -> Vec<Value> {
    let document = compiler_document();
    let contract = document.validated_contract().unwrap();
    Executor::new(&contract, compiler_limits())
        .unwrap()
        .run_graph("scheduler-order", vec![nodes, edges])
        .unwrap()
}
#[test]
fn g0_scheduler_emits_dependencies_before_users_even_when_ids_are_reversed() {
    let nodes = collection(&[
        &[1, 0, 0, 0, 0, 0, 0, 0],
        &[2, 0, 0, 0, 0, 0, 0, 0],
        &[3, 0, 0, 0, 0, 0, 0, 0],
    ]);
    let edges = collection(&[&[1, 2, 0, 0, 1, 0], &[1, 3, 0, 0, 1, 1]]);
    let output = schedule(nodes, edges);
    let [Value::Array(nodes)] = output.as_slice() else {
        panic!("ordered AST")
    };
    let ids: Vec<_> = nodes
        .iter()
        .map(|v| match v {
            Value::Array(fields) => fields[0].clone(),
            _ => panic!("node"),
        })
        .collect();
    assert_eq!(
        ids,
        vec![Value::Integer(2), Value::Integer(3), Value::Integer(1)]
    );
}
#[test]
fn g0_scheduler_rejects_cycles_instead_of_issuing_native_calls() {
    let nodes = collection(&[&[1, 0, 0, 0, 0, 0, 0, 0], &[2, 0, 0, 0, 0, 0, 0, 0]]);
    let edges = collection(&[&[1, 2, 0, 0, 1, 0], &[1, 1, 0, 0, 2, 0]]);
    assert_eq!(schedule(nodes, edges), vec![integers(&[])]);
}
