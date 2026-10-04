//! Linear G0 eligibility pass for graphs already ordered by the Builder.
use super::*;
fn rows() -> SemanticType {
    SemanticType::Slice(Box::new(SemanticType::Slice(Box::new(int()))))
}
fn call(g: &mut G, name: &str, args: Vec<SourceEndpoint>, output: SemanticType) -> SourceEndpoint {
    let args = args
        .into_iter()
        .map(|e| {
            let t = g.ty(&e);
            (e, t)
        })
        .collect();
    g.op(Operation::Subgraph(name.into()), args, output)
}
fn node_graphs() -> Vec<Graph> {
    let state = vec![rows(), int(), int(), SemanticType::Bool];
    let mut cond = G::new(
        "scheduler-fast-nodes-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let length = cond.length(input(0));
    let before = cond.compare(Operation::Lt, input(1), length);
    let test = cond.and(before, input(3));
    let mut body = G::new("scheduler-fast-nodes-body", state.clone(), state.clone());
    let node = body.index(input(0), input(1));
    let id = body.field(node, 0);
    let zero = body.n(0);
    let first = body.compare(Operation::Eq, input(1), zero);
    let after = body.compare(Operation::Gt, id.clone(), input(2));
    let ordered = body.logic(Operation::Or, first, after);
    let valid = body.and(input(3), ordered);
    let index = body.advance(input(1), 1);
    let mut main = G::new(
        "scheduler-fast-nodes",
        vec![rows()],
        vec![SemanticType::Bool],
    );
    let zero = main.n(0);
    let yes = main.bool(true);
    let out = main.loop_node(
        "scheduler-fast-nodes",
        state,
        vec![input(0), zero.clone(), zero, yes],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), index, id, valid]),
        main.finish(vec![out[3].clone()]),
    ]
}
fn edge_graphs() -> Vec<Graph> {
    let state = vec![rows(), int(), SemanticType::Bool];
    let mut cond = G::new(
        "scheduler-fast-edges-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let length = cond.length(input(0));
    let before = cond.compare(Operation::Lt, input(1), length);
    let test = cond.and(before, input(2));
    let mut body = G::new("scheduler-fast-edges-body", state.clone(), state.clone());
    let edge = body.index(input(0), input(1));
    let source_kind = body.field(edge.clone(), 0);
    let target_kind = body.field(edge.clone(), 3);
    let zero = body.n(0);
    let one = body.n(1);
    let source_node = body.compare(Operation::Eq, source_kind, one);
    let target_node = body.compare(Operation::Eq, target_kind, zero);
    let dependency = body.and(source_node, target_node);
    let ignored = body.not(dependency);
    let source = body.field(edge.clone(), 1);
    let target = body.field(edge, 4);
    let forward = body.compare(Operation::Lt, source, target);
    let safe = body.logic(Operation::Or, ignored, forward);
    let valid = body.and(input(2), safe);
    let index = body.advance(input(1), 1);
    let mut main = G::new(
        "scheduler-fast-edges",
        vec![rows()],
        vec![SemanticType::Bool],
    );
    let zero = main.n(0);
    let yes = main.bool(true);
    let out = main.loop_node("scheduler-fast-edges", state, vec![input(0), zero, yes]);
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), index, valid]),
        main.finish(vec![out[2].clone()]),
    ]
}
pub(super) fn graphs() -> Vec<Graph> {
    let mut graphs = node_graphs();
    graphs.extend(edge_graphs());
    let mut eligible = G::new(
        "scheduler-fast-eligible",
        vec![rows(), rows()],
        vec![SemanticType::Bool],
    );
    let ordered = call(
        &mut eligible,
        "scheduler-fast-nodes",
        vec![input(0)],
        SemanticType::Bool,
    );
    let forward = call(
        &mut eligible,
        "scheduler-fast-edges",
        vec![input(1)],
        SemanticType::Bool,
    );
    let yes = eligible.and(ordered, forward);
    graphs.push(eligible.finish(vec![yes]));
    let direct = G::new("scheduler-fast-direct", vec![rows(), rows()], vec![rows()]);
    graphs.push(direct.finish(vec![input(0)]));
    let mut fallback = G::new(
        "scheduler-fast-fallback",
        vec![rows(), rows()],
        vec![rows()],
    );
    let ordered = call(
        &mut fallback,
        "scheduler-order",
        vec![input(0), input(1)],
        rows(),
    );
    graphs.push(fallback.finish(vec![ordered]));
    let mut from_rows = G::new("scheduler-fast-rows", vec![rows(), rows()], vec![rows()]);
    let test = call(
        &mut from_rows,
        "scheduler-fast-eligible",
        vec![input(0), input(1)],
        SemanticType::Bool,
    );
    let result = from_rows.op(
        Operation::Select {
            when_true: "scheduler-fast-direct".into(),
            when_false: "scheduler-fast-fallback".into(),
        },
        vec![
            (test, SemanticType::Bool),
            (input(0), rows()),
            (input(1), rows()),
        ],
        rows(),
    );
    let node = from_rows.b.graph.nodes.last_mut().unwrap();
    node.inputs[0].name = "selector".into();
    node.inputs[1].name = "p0".into();
    node.inputs[2].name = "p1".into();
    graphs.push(from_rows.finish(vec![result]));
    let mut main = G::new(
        "scheduler-fast",
        vec![SemanticType::Bytes, int()],
        vec![rows()],
    );
    let ast = call(
        &mut main,
        "reader-graph-ast",
        vec![input(0), input(1)],
        rows(),
    );
    let edges = call(
        &mut main,
        "reader-graph-edges",
        vec![input(0), input(1)],
        rows(),
    );
    let result = call(&mut main, "scheduler-fast-rows", vec![ast, edges], rows());
    graphs.push(main.finish(vec![result]));
    graphs
}
