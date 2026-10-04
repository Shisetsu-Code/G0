//! Bounded native type shape; named references are proven by the program registry.
use super::*;
pub(super) fn graphs() -> Vec<Graph> {
    let ports_state = vec![SemanticType::Bytes, int(), int(), SemanticType::Bool];
    let mut pc = G::new(
        "validator-port-profile-condition",
        ports_state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = pc.n(0);
    let more_ports = pc.compare(Operation::Gt, input(2), zero);
    let more_ports = pc.and(more_ports, input(3));
    let mut pb = G::new(
        "validator-port-profile-body",
        ports_state.clone(),
        ports_state.clone(),
    );
    let name = pb.advance(input(1), 2);
    let ty = pb.skip_blob(name);
    let supported = call(
        &mut pb,
        "validator-type-profile",
        vec![input(0), ty],
        SemanticType::Bool,
    );
    let port_valid = pb.and(input(3), supported);
    let port_next = pb.call("reader-port-layout", input(1));
    let one = pb.n(1);
    let port_left = pb.arithmetic(Operation::Sub, input(2), one);
    let mut pm = G::new(
        "validator-port-profile",
        vec![SemanticType::Bytes, int()],
        vec![SemanticType::Bool],
    );
    let count = pm.u32(input(1));
    let start = pm.advance(input(1), 4);
    let yes = pm.bool(true);
    let ports = pm.loop_node(
        "validator-port-profile",
        ports_state,
        vec![input(0), start, count, yes],
    );
    let mut graphs = vec![
        pc.finish(vec![more_ports]),
        pb.finish(vec![input(0), port_next, port_left, port_valid]),
        pm.finish(vec![ports[3].clone()]),
    ];
    graphs.extend(schema_graphs());
    graphs
}

fn schema_graphs() -> Vec<Graph> {
    let state = vec![SemanticType::Bytes, int(), int()];
    let mut cond = G::new(
        "validator-schema-skip-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let more = cond.compare(Operation::Gt, input(2), zero);
    let mut body = G::new("validator-schema-skip-body", state.clone(), state.clone());
    let next = body.skip_blob(input(1));
    let one = body.n(1);
    let left = body.arithmetic(Operation::Sub, input(2), one);
    let mut main = G::new(
        "validator-empty-schemas",
        vec![SemanticType::Bytes],
        vec![SemanticType::Bool],
    );
    let eight = main.n(8);
    let list = main.skip_blob(eight);
    let count = main.u32(list.clone());
    let start = main.advance(list, 4);
    let result = main.loop_node("validator-schema-skip", state, vec![input(0), start, count]);
    let schema_count = main.u32(result[1].clone());
    let zero = main.n(0);
    let empty = main.compare(Operation::Eq, schema_count, zero);
    vec![
        cond.finish(vec![more]),
        body.finish(vec![input(0), next, left]),
        main.finish(vec![empty]),
    ]
}
