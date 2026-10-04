//! Structural semantic validation executed by GIR, over binary offset tables.
use super::*;
#[path = "bootstrap_compiler_typecheck.rs"]
mod typecheck;
fn rows() -> SemanticType {
    SemanticType::Slice(Box::new(SemanticType::Slice(Box::new(int()))))
}
fn call(
    g: &mut G,
    name: &str,
    values: Vec<SourceEndpoint>,
    output: SemanticType,
) -> SourceEndpoint {
    let args = values
        .into_iter()
        .map(|value| {
            let ty = match g.ty(&value) {
                SemanticType::Integer(_) => int(),
                other => other,
            };
            (value, ty)
        })
        .collect();
    g.op(Operation::Subgraph(name.into()), args, output)
}
fn select(
    g: &mut G,
    yes: &str,
    no: &str,
    test: SourceEndpoint,
    args: Vec<SourceEndpoint>,
) -> SourceEndpoint {
    let mut values = vec![(test, SemanticType::Bool)];
    for value in args {
        let ty = match g.ty(&value) {
            SemanticType::Integer(_) => int(),
            other => other,
        };
        values.push((value, ty));
    }
    let output = g.op(
        Operation::Select {
            when_true: yes.into(),
            when_false: no.into(),
        },
        values,
        SemanticType::Bool,
    );
    let node = g.b.graph.nodes.last_mut().unwrap();
    node.inputs[0].name = "selector".into();
    for (index, port) in node.inputs.iter_mut().skip(1).enumerate() {
        port.name = format!("p{index}");
    }
    output
}
fn port_id(g: &mut G, at: SourceEndpoint) -> SourceEndpoint {
    let word = g.u32(at);
    let modulus = g.n(65536);
    g.op(Operation::Rem, vec![(word, int()), (modulus, int())], int())
}
fn endpoint_graphs() -> Vec<Graph> {
    // bytes, node table, graph port-list offset, endpoint kind/node/port,
    // node port-list descriptor field, node endpoint discriminator.
    let types = vec![
        SemanticType::Bytes,
        rows(),
        int(),
        int(),
        int(),
        int(),
        int(),
        int(),
    ];
    let mut graph = G::new(
        "validator-endpoint-graph",
        types.clone(),
        vec![SemanticType::Bool],
    );
    let rank = call(
        &mut graph,
        "control-port-rank",
        vec![input(0), input(2), input(5)],
        int(),
    );
    let count = graph.u32(input(2));
    let graph_valid = graph.compare(Operation::Lt, rank, count);
    let mut extended = types.clone();
    extended.push(int());
    let mut ports = G::new(
        "validator-endpoint-node-ports",
        extended.clone(),
        vec![SemanticType::Bool],
    );
    let node = ports.index(input(1), input(8));
    let list = ports.index(node, input(6));
    let rank = call(
        &mut ports,
        "control-port-rank",
        vec![input(0), list.clone(), input(5)],
        int(),
    );
    let count = ports.u32(list);
    let port_valid = ports.compare(Operation::Lt, rank, count);
    let mut invalid = G::new(
        "validator-endpoint-invalid",
        extended,
        vec![SemanticType::Bool],
    );
    let no = invalid.bool(false);
    let mut node = G::new(
        "validator-endpoint-node",
        types.clone(),
        vec![SemanticType::Bool],
    );
    let index = call(
        &mut node,
        "emitter-node-index",
        vec![input(1), input(4)],
        int(),
    );
    let count = node.length(input(1));
    let found = node.compare(Operation::Lt, index.clone(), count);
    let mut args: Vec<_> = (0..8).map(input).collect();
    args.push(index);
    let node_valid = select(
        &mut node,
        "validator-endpoint-node-ports",
        "validator-endpoint-invalid",
        found,
        args,
    );
    let mut main = G::new("validator-endpoint", types, vec![SemanticType::Bool]);
    let is_node = main.compare(Operation::Eq, input(3), input(7));
    let valid = select(
        &mut main,
        "validator-endpoint-node",
        "validator-endpoint-graph",
        is_node,
        (0..8).map(input).collect(),
    );
    vec![
        graph.finish(vec![graph_valid]),
        ports.finish(vec![port_valid]),
        invalid.finish(vec![no]),
        node.finish(vec![node_valid]),
        main.finish(vec![valid]),
    ]
}
fn incoming_graphs() -> Vec<Graph> {
    let types = vec![rows(), int(), int(), int()];
    let mut state = types.clone();
    state.extend([int(), int()]);
    let mut cond = G::new(
        "validator-incoming-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let length = cond.length(input(0));
    let test = cond.compare(Operation::Lt, input(4), length);
    let mut body = G::new("validator-incoming-body", state.clone(), state.clone());
    let edge = body.index(input(0), input(4));
    let mut matches = body.bool(true);
    for (field, parameter) in [(3, 1), (4, 2), (5, 3)] {
        let value = body.field(edge.clone(), field);
        let same = body.compare(Operation::Eq, value, input(parameter));
        matches = body.and(matches, same);
    }
    let increased = body.advance(input(5), 1);
    let count = body.pick("reader-pick-offset", matches, input(5), increased);
    let next = body.advance(input(4), 1);
    let mut main = G::new("validator-incoming", types, vec![int()]);
    let zero = main.n(0);
    let out = main.loop_node(
        "validator-incoming",
        state,
        vec![input(0), input(1), input(2), input(3), zero.clone(), zero],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), input(2), input(3), next, count]),
        main.finish(vec![out[5].clone()]),
    ]
}
fn required_port_graphs() -> Vec<Graph> {
    let state = vec![
        SemanticType::Bytes,
        rows(),
        int(),
        int(),
        int(),
        int(),
        SemanticType::Bool,
    ];
    let mut cond = G::new(
        "validator-required-ports-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let test = cond.compare(Operation::Gt, input(3), zero);
    let test = cond.and(test, input(6));
    let mut body = G::new(
        "validator-required-ports-body",
        state.clone(),
        state.clone(),
    );
    let id = port_id(&mut body, input(2));
    let count = call(
        &mut body,
        "validator-incoming",
        vec![input(1), input(4), input(5), id],
        int(),
    );
    let one = body.n(1);
    let once = body.compare(Operation::Eq, count, one.clone());
    let valid = body.and(input(6), once);
    let next = body.call("reader-port", input(2));
    let left = body.arithmetic(Operation::Sub, input(3), one);
    let types = vec![SemanticType::Bytes, rows(), int(), int(), int()];
    let mut main = G::new("validator-required-ports", types, vec![SemanticType::Bool]);
    let count = main.u32(input(2));
    let start = main.advance(input(2), 4);
    let yes = main.bool(true);
    let out = main.loop_node(
        "validator-required-ports",
        state,
        vec![input(0), input(1), start, count, input(3), input(4), yes],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![
            input(0),
            input(1),
            next,
            left,
            input(4),
            input(5),
            valid,
        ]),
        main.finish(vec![out[6].clone()]),
    ]
}
fn node_inputs_graphs() -> Vec<Graph> {
    let types = vec![SemanticType::Bytes, rows(), rows()];
    let mut state = types.clone();
    state.extend([int(), SemanticType::Bool]);
    let mut cond = G::new(
        "validator-node-inputs-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let count = cond.length(input(1));
    let test = cond.compare(Operation::Lt, input(3), count);
    let test = cond.and(test, input(4));
    let mut body = G::new("validator-node-inputs-body", state.clone(), state.clone());
    let node = body.index(input(1), input(3));
    let id = body.field(node.clone(), 0);
    let ports = body.field(node, 3);
    let zero = body.n(0);
    let valid = call(
        &mut body,
        "validator-required-ports",
        vec![input(0), input(2), ports, zero, id],
        SemanticType::Bool,
    );
    let valid = body.and(input(4), valid);
    let next = body.advance(input(3), 1);
    let mut main = G::new("validator-node-inputs", types, vec![SemanticType::Bool]);
    let zero = main.n(0);
    let yes = main.bool(true);
    let out = main.loop_node(
        "validator-node-inputs",
        state,
        vec![input(0), input(1), input(2), zero, yes],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), input(2), next, valid]),
        main.finish(vec![out[4].clone()]),
    ]
}
fn edge_graphs() -> Vec<Graph> {
    let types = vec![SemanticType::Bytes, rows(), rows(), int(), int()];
    let mut state = types.clone();
    state.extend([int(), SemanticType::Bool]);
    let mut cond = G::new(
        "validator-edges-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let count = cond.length(input(2));
    let test = cond.compare(Operation::Lt, input(5), count);
    let test = cond.and(test, input(6));
    let mut body = G::new("validator-edges-body", state.clone(), state.clone());
    let edge = body.index(input(2), input(5));
    let mut valid = input(6);
    for (start, list, field, node_kind) in [(0, 3, 4, 1), (3, 4, 3, 0)] {
        let kind = body.field(edge.clone(), start);
        let node = body.field(edge.clone(), start + 1);
        let port = body.field(edge.clone(), start + 2);
        let field = body.n(field);
        let node_kind = body.n(node_kind);
        let endpoint_valid = call(
            &mut body,
            "validator-endpoint",
            vec![
                input(0),
                input(1),
                input(list),
                kind,
                node,
                port,
                field,
                node_kind,
            ],
            SemanticType::Bool,
        );
        valid = body.and(valid, endpoint_valid);
    }
    let edge_valid = valid;
    let next = body.advance(input(5), 1);
    let mut main = G::new("validator-edges", types, vec![SemanticType::Bool]);
    let zero = main.n(0);
    let yes = main.bool(true);
    let out = main.loop_node(
        "validator-edges",
        state,
        vec![input(0), input(1), input(2), input(3), input(4), zero, yes],
    );
    let mut graph = G::new(
        "validator-graph-edges",
        vec![SemanticType::Bytes, int()],
        vec![SemanticType::Bool],
    );
    let name = graph.advance(input(1), 8);
    let authority = graph.skip_blob(name);
    let inputs = graph.advance(authority, 1);
    let outputs = graph.call("reader-list-reader-port", inputs.clone());
    let nodes = call(
        &mut graph,
        "reader-graph-ast",
        vec![input(0), input(1)],
        rows(),
    );
    let edges = call(
        &mut graph,
        "reader-graph-edges",
        vec![input(0), input(1)],
        rows(),
    );
    let endpoints = call(
        &mut graph,
        "validator-edges",
        vec![
            input(0),
            nodes.clone(),
            edges.clone(),
            inputs,
            outputs.clone(),
        ],
        SemanticType::Bool,
    );
    let node_inputs = call(
        &mut graph,
        "validator-node-inputs",
        vec![input(0), nodes, edges.clone()],
        SemanticType::Bool,
    );
    let one = graph.n(1);
    let zero = graph.n(0);
    let graph_outputs = call(
        &mut graph,
        "validator-required-ports",
        vec![input(0), edges, outputs, one, zero],
        SemanticType::Bool,
    );
    let graph_valid = graph.and(endpoints, node_inputs);
    let graph_valid = graph.and(graph_valid, graph_outputs);
    vec![
        cond.finish(vec![test]),
        body.finish(vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            next,
            edge_valid,
        ]),
        main.finish(vec![out[6].clone()]),
        graph.finish(vec![graph_valid]),
    ]
}
pub(super) fn graphs() -> Vec<Graph> {
    let mut graphs = endpoint_graphs();
    graphs.extend(incoming_graphs());
    graphs.extend(required_port_graphs());
    graphs.extend(node_inputs_graphs());
    graphs.extend(edge_graphs());
    graphs.extend(typecheck::graphs());
    graphs
}
