//! Structural semantic validation executed by GIR, over binary offset tables.
use super::*;
#[path = "bootstrap_compiler_linkcheck.rs"]
mod linkcheck;
#[path = "bootstrap_compiler_schemas.rs"]
mod schemas;
#[path = "bootstrap_compiler_type_depth.rs"]
mod type_depth;
#[path = "bootstrap_compiler_type_profile.rs"]
mod type_profile;
#[path = "bootstrap_compiler_typecheck.rs"]
mod typecheck;
#[path = "bootstrap_compiler_wires.rs"]
mod wires;
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
fn port_position_graphs() -> Vec<Graph> {
    let loop_state = vec![SemanticType::Bytes, int(), int()];
    let mut cond = G::new(
        "validator-port-cursor-condition",
        loop_state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let test = cond.compare(Operation::Gt, input(2), zero);
    let mut body = G::new(
        "validator-port-cursor-body",
        loop_state.clone(),
        loop_state.clone(),
    );
    let body_cursor = body.call("reader-port-layout", input(1));
    let one = body.n(1);
    let left = body.arithmetic(Operation::Sub, input(2), one);
    let mut main = G::new("validator-port-cursor", loop_state.clone(), vec![int()]);
    let start = main.advance(input(1), 4);
    let out = main.loop_node(
        "validator-port-cursor",
        loop_state.clone(),
        vec![input(0), start, input(2)],
    );
    let mut ty = G::new("validator-port-type-at", loop_state.clone(), vec![int()]);
    let cursor = call(
        &mut ty,
        "validator-port-cursor",
        vec![input(0), input(1), input(2)],
        int(),
    );
    let name = ty.advance(cursor, 2);
    let type_at = ty.skip_blob(name);
    let mut id = G::new("validator-port-id-at", loop_state, vec![int()]);
    let cursor = call(
        &mut id,
        "validator-port-cursor",
        vec![input(0), input(1), input(2)],
        int(),
    );
    let port = port_id(&mut id, cursor);
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), body_cursor, left]),
        main.finish(vec![out[1].clone()]),
        ty.finish(vec![type_at]),
        id.finish(vec![port]),
    ]
}
fn endpoint_type_graphs() -> Vec<Graph> {
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
    let graph = G::new("validator-endpoint-graph-list", types.clone(), vec![int()]);
    let mut node = G::new("validator-endpoint-node-list", types.clone(), vec![int()]);
    let index = call(
        &mut node,
        "validator-node-index-fast",
        vec![input(1), input(4)],
        int(),
    );
    let descriptor = node.index(input(1), index);
    let node_list = node.index(descriptor, input(6));
    let mut main = G::new("validator-endpoint-type", types, vec![int()]);
    let is_node = main.compare(Operation::Eq, input(3), input(7));
    let mut args = vec![(is_node, SemanticType::Bool)];
    args.extend((0..8).map(|p| (input(p), main.b.graph.inputs[p as usize].ty.clone())));
    let list = main.op(
        Operation::Select {
            when_true: "validator-endpoint-node-list".into(),
            when_false: "validator-endpoint-graph-list".into(),
        },
        args,
        int(),
    );
    let selector = main.b.graph.nodes.last_mut().unwrap();
    selector.inputs[0].name = "selector".into();
    for (i, port) in selector.inputs.iter_mut().skip(1).enumerate() {
        port.name = format!("p{i}");
    }
    let rank = call(
        &mut main,
        "validator-port-rank-fast",
        vec![input(0), list.clone(), input(5)],
        int(),
    );
    let type_at = call(
        &mut main,
        "validator-port-type-at",
        vec![input(0), list, rank],
        int(),
    );
    vec![
        graph.finish(vec![input(2)]),
        node.finish(vec![node_list]),
        main.finish(vec![type_at]),
    ]
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
        "validator-port-rank-fast",
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
        "validator-port-rank-fast",
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
        "validator-node-index-fast",
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
    let mut endpoint_types = Vec::new();
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
                kind.clone(),
                node.clone(),
                port.clone(),
                field.clone(),
                node_kind.clone(),
            ],
            SemanticType::Bool,
        );
        valid = body.and(valid, endpoint_valid);
        let type_at = call(
            &mut body,
            "validator-endpoint-type",
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
            int(),
        );
        endpoint_types.push(type_at);
    }
    let assigned = call(
        &mut body,
        "validator-type-assignable",
        vec![
            input(0),
            endpoint_types[0].clone(),
            endpoint_types[1].clone(),
        ],
        SemanticType::Bool,
    );
    valid = body.and(valid, assigned);
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
            inputs.clone(),
            outputs.clone(),
        ],
        SemanticType::Bool,
    );
    let scheduled = call(
        &mut graph,
        "scheduler-fast-rows",
        vec![nodes.clone(), edges.clone()],
        rows(),
    );
    let scheduled_count = graph.length(scheduled);
    let node_count = graph.length(nodes.clone());
    let acyclic = graph.compare(Operation::Eq, scheduled_count, node_count);
    let graph_valid = select(
        &mut graph,
        "validator-wire-complete",
        "validator-wire-invalid",
        endpoints,
        vec![input(0), nodes.clone(), edges, inputs, outputs],
    );
    let mut invalid_wires = G::new(
        "validator-wire-invalid",
        vec![SemanticType::Bytes, rows(), rows(), int(), int()],
        vec![SemanticType::Bool],
    );
    let no = invalid_wires.bool(false);
    let operations = call(
        &mut graph,
        "validator-operations",
        vec![input(0), nodes],
        SemanticType::Bool,
    );
    let graph_valid = graph.and(graph_valid, operations);
    let graph_valid = graph.and(graph_valid, acyclic);
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
        invalid_wires.finish(vec![no]),
    ]
}
fn operation_graphs() -> Vec<Graph> {
    let seq = SemanticType::Slice(Box::new(int()));
    let args = vec![SemanticType::Bytes, seq.clone()];
    let mut skip = G::new(
        "validator-operation-control",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let yes = skip.bool(true);
    let mut node = G::new("validator-operation-local", args, vec![SemanticType::Bool]);
    let at = node.field(input(1), 2);
    let tag = node.byte(at);
    let four = node.n(4);
    let seven = node.n(7);
    let lower = node.compare(Operation::Ge, tag.clone(), four);
    let upper = node.compare(Operation::Le, tag.clone(), seven);
    let control = node.and(lower, upper);
    let fortyseven = node.n(47);
    let call_node = node.compare(Operation::Eq, tag.clone(), fortyseven);
    let control = node.op(
        Operation::Or,
        vec![
            (control, SemanticType::Bool),
            (call_node, SemanticType::Bool),
        ],
        SemanticType::Bool,
    );
    let mut deferred = control;
    for (low, high) in [(8, 16), (49, 65)] {
        let low = node.n(low);
        let high = node.n(high);
        let a = node.compare(Operation::Ge, tag.clone(), low);
        let b = node.compare(Operation::Le, tag.clone(), high);
        let range = node.and(a, b);
        deferred = node.op(
            Operation::Or,
            vec![(deferred, SemanticType::Bool), (range, SemanticType::Bool)],
            SemanticType::Bool,
        );
    }
    let local = select(
        &mut node,
        "validator-operation-control",
        "validator-node-operation",
        deferred,
        vec![input(0), input(1)],
    );
    let state = vec![SemanticType::Bytes, rows(), int(), SemanticType::Bool];
    let mut cond = G::new(
        "validator-operations-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(1));
    let more = cond.compare(Operation::Lt, input(2), len);
    let more = cond.and(more, input(3));
    let mut body = G::new("validator-operations-body", state.clone(), state.clone());
    let row = body.index(input(1), input(2));
    let valid = call(
        &mut body,
        "validator-operation-local",
        vec![input(0), row],
        SemanticType::Bool,
    );
    let valid = body.and(input(3), valid);
    let one = body.n(1);
    let next = body.arithmetic(Operation::Add, input(2), one);
    let mut main = G::new(
        "validator-operations",
        vec![SemanticType::Bytes, rows()],
        vec![SemanticType::Bool],
    );
    let zero = main.n(0);
    let yes_main = main.bool(true);
    let out = main.loop_node(
        "validator-operations",
        state,
        vec![input(0), input(1), zero, yes_main],
    );
    vec![
        skip.finish(vec![yes]),
        node.finish(vec![local]),
        cond.finish(vec![more]),
        body.finish(vec![input(0), input(1), next, valid]),
        main.finish(vec![out[3].clone()]),
    ]
}
pub(super) fn graphs() -> Vec<Graph> {
    let mut graphs = endpoint_graphs();
    graphs.extend(port_position_graphs());
    graphs.extend(endpoint_type_graphs());
    graphs.extend(incoming_graphs());
    graphs.extend(required_port_graphs());
    graphs.extend(node_inputs_graphs());
    graphs.extend(edge_graphs());
    graphs.extend(typecheck::graphs());
    graphs.extend(linkcheck::graphs());
    graphs.extend(operation_graphs());
    graphs.extend(wires::graphs());
    graphs.extend(type_profile::graphs());
    graphs.extend(schemas::graphs());
    graphs.extend(type_depth::graphs());
    graphs
}
