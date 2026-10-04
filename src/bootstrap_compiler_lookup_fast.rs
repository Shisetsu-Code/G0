//! G0 dense lookup shortcuts with verified candidates and general fallbacks.
use super::*;

fn rows() -> SemanticType {
    SemanticType::Slice(Box::new(SemanticType::Slice(Box::new(int()))))
}

fn call(g: &mut G, name: &str, args: Vec<SourceEndpoint>, output: SemanticType) -> SourceEndpoint {
    let args = args
        .into_iter()
        .map(|arg| {
            let ty = g.ty(&arg);
            (arg, ty)
        })
        .collect();
    g.op(Operation::Subgraph(name.into()), args, output)
}

fn choose(
    g: &mut G,
    test: SourceEndpoint,
    yes: &str,
    no: &str,
    values: Vec<SourceEndpoint>,
) -> SourceEndpoint {
    let mut args = vec![(test, SemanticType::Bool)];
    args.extend(values.into_iter().map(|value| {
        let ty = g.ty(&value);
        (value, ty)
    }));
    let result = g.op(
        Operation::Select {
            when_true: yes.into(),
            when_false: no.into(),
        },
        args,
        int(),
    );
    let ports = &mut g.b.graph.nodes.last_mut().unwrap().inputs;
    ports[0].name = "selector".into();
    for (i, port) in ports.iter_mut().skip(1).enumerate() {
        port.name = format!("p{i}");
    }
    result
}

pub(super) fn graphs() -> Vec<Graph> {
    let branch_types = vec![rows(), int(), int()];
    let dense = G::new(
        "validator-node-index-dense",
        branch_types.clone(),
        vec![int()],
    );
    let mut fallback = G::new("validator-node-index-fallback", branch_types, vec![int()]);
    let result = call(
        &mut fallback,
        "emitter-node-index",
        vec![input(0), input(1)],
        int(),
    );
    let mut node = G::new(
        "validator-node-index-fast",
        vec![rows(), int()],
        vec![int()],
    );
    let zero = node.n(0);
    let one = node.n(1);
    let positive = node.compare(Operation::Gt, input(1), zero);
    // id zero remains valid through the general lookup; never subtract it.
    let safe = node.pick(
        "reader-pick-offset",
        positive.clone(),
        one.clone(),
        input(1),
    );
    let candidate = node.arithmetic(Operation::Sub, safe, one);
    let length = node.length(input(0));
    let inside = node.compare(Operation::Lt, candidate.clone(), length);
    let descriptor = node.index(input(0), candidate.clone());
    let id = node.field(descriptor, 0);
    let equal = node.compare(Operation::Eq, id, input(1));
    let valid = node.and(positive, inside);
    let valid = node.and(valid, equal);
    let result_node = choose(
        &mut node,
        valid,
        "validator-node-index-dense",
        "validator-node-index-fallback",
        vec![input(0), input(1), candidate],
    );

    let port_types = vec![SemanticType::Bytes, int(), int()];
    let dense_port = G::new("validator-port-rank-dense", port_types.clone(), vec![int()]);
    let mut fallback_port = G::new(
        "validator-port-rank-fallback",
        port_types.clone(),
        vec![int()],
    );
    let fallback_rank = call(
        &mut fallback_port,
        "control-port-rank",
        vec![input(0), input(1), input(2)],
        int(),
    );
    let mut candidate_port = G::new(
        "validator-port-rank-candidate",
        port_types.clone(),
        vec![int()],
    );
    let id = call(
        &mut candidate_port,
        "validator-port-id-at",
        vec![input(0), input(1), input(2)],
        int(),
    );
    let matched = candidate_port.compare(Operation::Eq, id, input(2));
    let rank = choose(
        &mut candidate_port,
        matched,
        "validator-port-rank-dense",
        "validator-port-rank-fallback",
        vec![input(0), input(1), input(2)],
    );
    let mut port = G::new("validator-port-rank-fast", port_types, vec![int()]);
    let count = port.u32(input(1));
    let inside = port.compare(Operation::Lt, input(2), count);
    let result_port = choose(
        &mut port,
        inside,
        "validator-port-rank-candidate",
        "validator-port-rank-fallback",
        vec![input(0), input(1), input(2)],
    );
    vec![
        dense.finish(vec![input(2)]),
        fallback.finish(vec![result]),
        node.finish(vec![result_node]),
        dense_port.finish(vec![input(2)]),
        fallback_port.finish(vec![fallback_rank]),
        candidate_port.finish(vec![rank]),
        port.finish(vec![result_port]),
    ]
}
