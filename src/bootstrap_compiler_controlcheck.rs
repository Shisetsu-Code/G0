//! Graph reference and exact control signatures proved by executable G0.
//! The native profile's global domain gate separately proves that every graph
//! has no effects/capabilities; this avoids rescanning callee bodies per call.
use super::*;
fn row_type() -> SemanticType {
    SemanticType::Slice(Box::new(int()))
}
fn rows() -> SemanticType {
    SemanticType::Slice(Box::new(row_type()))
}
fn node_args() -> Vec<SemanticType> {
    vec![SemanticType::Bytes, rows(), row_type()]
}
fn call(g: &mut G, name: &str, args: Vec<SourceEndpoint>, output: SemanticType) -> SourceEndpoint {
    let args = args
        .into_iter()
        .map(|e| {
            let mut t = g.ty(&e);
            if matches!(&t,SemanticType::Integer(r)if r.min>=0&&r.max<=1_i128<<48) {
                t = int();
            }
            (e, t)
        })
        .collect();
    g.op(Operation::Subgraph(name.into()), args, output)
}
fn choose(
    g: &mut G,
    test: SourceEndpoint,
    yes: &str,
    no: &str,
    args: Vec<SourceEndpoint>,
    output: SemanticType,
) -> SourceEndpoint {
    let mut values = vec![(test, SemanticType::Bool)];
    values.extend(args.into_iter().map(|e| {
        let mut t = g.ty(&e);
        if matches!(&t,SemanticType::Integer(r)if r.min>=0&&r.max<=1_i128<<48) {
            t = int();
        }
        (e, t)
    }));
    let value = g.op(
        Operation::Select {
            when_true: yes.into(),
            when_false: no.into(),
        },
        values,
        output,
    );
    let node = g.b.graph.nodes.last_mut().unwrap();
    node.inputs[0].name = "selector".into();
    for (i, p) in node.inputs[1..].iter_mut().enumerate() {
        p.name = format!("p{i}");
    }
    value
}
fn eq(g: &mut G, value: SourceEndpoint, n: i128) -> SourceEndpoint {
    let n = g.n(n);
    g.compare(Operation::Eq, value, n)
}
fn type_at(g: &mut G, list: SourceEndpoint, ordinal: SourceEndpoint) -> SourceEndpoint {
    call(
        g,
        "validator-port-type-at",
        vec![input(0), list, ordinal],
        int(),
    )
}
fn kind(g: &mut G, at: SourceEndpoint, tag: i128) -> SourceEndpoint {
    let value = g.byte(at);
    eq(g, value, tag)
}
fn all(g: &mut G, flags: Vec<SourceEndpoint>) -> SourceEndpoint {
    flags
        .into_iter()
        .reduce(|a, b| g.and(a, b))
        .unwrap_or_else(|| g.bool(true))
}
fn operation(g: &mut G) -> SourceEndpoint {
    g.field(input(2), 2)
}
fn node_list(g: &mut G, field: i128) -> SourceEndpoint {
    g.field(input(2), field)
}
fn name_graphs() -> Vec<Graph> {
    let state = vec![
        SemanticType::Bytes,
        rows(),
        int(),
        int(),
        SemanticType::Bool,
    ];
    let mut cond = G::new(
        "controlcheck-name-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(1));
    let more = cond.compare(Operation::Lt, input(3), len);
    let unseen = cond.not(input(4));
    let test = cond.and(more, unseen);
    let mut body = G::new("controlcheck-name-body", state.clone(), state.clone());
    let graph = body.index(input(1), input(3));
    let name = body.field(graph, 2);
    let same = call(
        &mut body,
        "validator-name-equal",
        vec![input(0), input(2), name],
        SemanticType::Bool,
    );
    let next = body.advance(input(3), 1);
    let next = body.pick("reader-pick-offset", same.clone(), next, input(3));
    let mut main = G::new(
        "validator-graph-name-index",
        vec![SemanticType::Bytes, rows(), int()],
        vec![int()],
    );
    let zero = main.n(0);
    let no = main.bool(false);
    let out = main.loop_node(
        "controlcheck-name",
        state,
        vec![input(0), input(1), input(2), zero, no],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), input(2), next, same]),
        main.finish(vec![out[3].clone()]),
    ]
}
fn type_graph() -> Graph {
    let mut g = G::new(
        "controlcheck-type-exact",
        vec![SemanticType::Bytes, int(), int()],
        vec![SemanticType::Bool],
    );
    let a_end = call(
        &mut g,
        "reader-type-layout",
        vec![input(0), input(1)],
        int(),
    );
    let b_end = call(
        &mut g,
        "reader-type-layout",
        vec![input(0), input(2)],
        int(),
    );
    let a_len = g.arithmetic(Operation::Sub, a_end, input(1));
    let b_len = g.arithmetic(Operation::Sub, b_end, input(2));
    let same_len = g.compare(Operation::Eq, a_len.clone(), b_len.clone());
    let smaller = g.compare(Operation::Lt, a_len.clone(), b_len.clone());
    let length = g.pick("reader-pick-offset", smaller, b_len, a_len);
    let bytes = call(
        &mut g,
        "validator-byte-equal",
        vec![input(0), input(1), input(2), length],
        SemanticType::Bool,
    );
    let valid = g.and(same_len, bytes);
    g.finish(vec![valid])
}
fn port_graphs() -> Vec<Graph> {
    let args = vec![
        SemanticType::Bytes,
        int(),
        int(),
        int(),
        int(),
        SemanticType::Bool,
    ];
    let state = vec![
        SemanticType::Bytes,
        int(),
        int(),
        int(),
        int(),
        SemanticType::Bool,
        int(),
        SemanticType::Bool,
    ];
    let mut cond = G::new(
        "controlcheck-ports-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let more = cond.compare(Operation::Gt, input(6), zero);
    let test = cond.and(more, input(7));
    let mut body = G::new("controlcheck-ports-body", state.clone(), state.clone());
    let a = type_at(&mut body, input(1), input(2));
    let b = type_at(&mut body, input(3), input(4));
    let same_type = call(
        &mut body,
        "controlcheck-type-exact",
        vec![input(0), a, b],
        SemanticType::Bool,
    );
    let a_cursor = call(
        &mut body,
        "validator-port-cursor",
        vec![input(0), input(1), input(2)],
        int(),
    );
    let b_cursor = call(
        &mut body,
        "validator-port-cursor",
        vec![input(0), input(3), input(4)],
        int(),
    );
    let a_name = body.advance(a_cursor, 2);
    let b_name = body.advance(b_cursor, 2);
    let same_names = call(
        &mut body,
        "validator-name-equal",
        vec![input(0), a_name, b_name],
        SemanticType::Bool,
    );
    let ignore_names = body.not(input(5));
    let names = body.logic(Operation::Or, ignore_names, same_names);
    let body_valid = all(&mut body, vec![input(7), same_type, names]);
    let next_a = body.advance(input(2), 1);
    let next_b = body.advance(input(4), 1);
    let one = body.n(1);
    let left = body.arithmetic(Operation::Sub, input(6), one);
    let mut known = G::new(
        "controlcheck-ports-known",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let a_count = known.u32(input(1));
    let b_count = known.u32(input(3));
    let a_count = known.arithmetic(Operation::Sub, a_count, input(2));
    let b_count = known.arithmetic(Operation::Sub, b_count, input(4));
    let sizes = known.compare(Operation::Eq, a_count.clone(), b_count);
    let out = known.loop_node(
        "controlcheck-ports",
        state,
        vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            input(5),
            a_count,
            sizes,
        ],
    );
    let mut invalid = G::new(
        "controlcheck-ports-invalid",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let no = invalid.bool(false);
    let mut main = G::new("controlcheck-ports", args, vec![SemanticType::Bool]);
    let a_count = main.u32(input(1));
    let b_count = main.u32(input(3));
    let enough_a = main.compare(Operation::Ge, a_count, input(2));
    let enough_b = main.compare(Operation::Ge, b_count, input(4));
    let enough = main.and(enough_a, enough_b);
    let valid = choose(
        &mut main,
        enough,
        "controlcheck-ports-known",
        "controlcheck-ports-invalid",
        vec![input(0), input(1), input(2), input(3), input(4), input(5)],
        SemanticType::Bool,
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![
            input(0),
            input(1),
            next_a,
            input(3),
            next_b,
            input(5),
            left,
            body_valid,
        ]),
        known.finish(vec![out[7].clone()]),
        invalid.finish(vec![no]),
        main.finish(vec![valid]),
    ]
}
fn target_args() -> Vec<SemanticType> {
    let mut args = node_args();
    args.extend([int(), int(), SemanticType::Bool, int()]);
    args
}
fn target_graphs() -> Vec<Graph> {
    let args = target_args();
    let mut known = G::new(
        "controlcheck-target-known",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let target = known.index(input(1), input(6));
    let target_inputs = known.field(target.clone(), 3);
    let target_outputs = known.field(target, 4);
    let ni = node_list(&mut known, 3);
    let no = node_list(&mut known, 4);
    let zero = known.n(0);
    let inputs = call(
        &mut known,
        "controlcheck-ports",
        vec![
            input(0),
            ni,
            input(4),
            target_inputs,
            zero.clone(),
            input(5),
        ],
        SemanticType::Bool,
    );
    let outputs = call(
        &mut known,
        "controlcheck-ports",
        vec![input(0), no, zero.clone(), target_outputs, zero, input(5)],
        SemanticType::Bool,
    );
    let ok = known.and(inputs, outputs);
    let mut invalid = G::new(
        "controlcheck-target-invalid",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let no = invalid.bool(false);
    let mut main = G::new(
        "controlcheck-target",
        args[..6].to_vec(),
        vec![SemanticType::Bool],
    );
    let index = call(
        &mut main,
        "validator-graph-name-index",
        vec![input(0), input(1), input(3)],
        int(),
    );
    let len = main.length(input(1));
    let found = main.compare(Operation::Lt, index.clone(), len);
    let valid = choose(
        &mut main,
        found,
        "controlcheck-target-known",
        "controlcheck-target-invalid",
        vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            input(5),
            index,
        ],
        SemanticType::Bool,
    );
    vec![
        known.finish(vec![ok]),
        invalid.finish(vec![no]),
        main.finish(vec![valid]),
    ]
}
fn basic_graphs() -> Vec<Graph> {
    let mut sub = G::new(
        "controlcheck-subgraph",
        node_args(),
        vec![SemanticType::Bool],
    );
    let op = operation(&mut sub);
    let name = sub.advance(op, 1);
    let zero = sub.n(0);
    let no = sub.bool(false);
    let valid_sub = call(
        &mut sub,
        "controlcheck-target",
        vec![input(0), input(1), input(2), name, zero, no],
        SemanticType::Bool,
    );
    let mut select = G::new(
        "controlcheck-select-known",
        node_args(),
        vec![SemanticType::Bool],
    );
    let op = operation(&mut select);
    let yes = select.advance(op, 1);
    let no = select.skip_blob(yes.clone());
    let one = select.n(1);
    let names = select.bool(true);
    let a = call(
        &mut select,
        "controlcheck-target",
        vec![
            input(0),
            input(1),
            input(2),
            yes,
            one.clone(),
            names.clone(),
        ],
        SemanticType::Bool,
    );
    let b = call(
        &mut select,
        "controlcheck-target",
        vec![input(0), input(1), input(2), no, one, names],
        SemanticType::Bool,
    );
    let valid_select = select.and(a, b);
    let mut guard = G::new("controlcheck-select", node_args(), vec![SemanticType::Bool]);
    let list = node_list(&mut guard, 3);
    let n = guard.u32(list);
    let zero = guard.n(0);
    let enough = guard.compare(Operation::Gt, n, zero);
    let valid_guard = choose(
        &mut guard,
        enough,
        "controlcheck-select-selector",
        "controlcheck-invalid",
        vec![input(0), input(1), input(2)],
        SemanticType::Bool,
    );
    let mut selector = G::new(
        "controlcheck-select-selector",
        node_args(),
        vec![SemanticType::Bool],
    );
    let list = node_list(&mut selector, 3);
    let zero = selector.n(0);
    let first = type_at(&mut selector, list, zero);
    let bool_selector = kind(&mut selector, first, 0);
    let valid_selector = choose(
        &mut selector,
        bool_selector,
        "controlcheck-select-known",
        "controlcheck-invalid",
        vec![input(0), input(1), input(2)],
        SemanticType::Bool,
    );
    vec![
        sub.finish(vec![valid_sub]),
        select.finish(vec![valid_select]),
        guard.finish(vec![valid_guard]),
        selector.finish(vec![valid_selector]),
    ]
}
fn indexed_args() -> Vec<SemanticType> {
    let mut args = node_args();
    args.push(int());
    args
}
fn loop_graphs() -> Vec<Graph> {
    let args = indexed_args();
    let mut invalid = G::new(
        "controlcheck-index-invalid",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let no = invalid.bool(false);
    let mut condition_type = G::new(
        "controlcheck-condition-type",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let graph = condition_type.index(input(1), input(3));
    let outputs = condition_type.field(graph, 4);
    let zero = condition_type.n(0);
    let ty = type_at(&mut condition_type, outputs, zero);
    let is_bool = kind(&mut condition_type, ty, 0);
    let mut condition = G::new(
        "controlcheck-condition",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let graph = condition.index(input(1), input(3));
    let callee_inputs = condition.field(graph.clone(), 3);
    let callee_outputs = condition.field(graph, 4);
    let inputs = node_list(&mut condition, 3);
    let zero = condition.n(0);
    let names = condition.bool(true);
    let input_ok = call(
        &mut condition,
        "controlcheck-ports",
        vec![input(0), inputs, zero.clone(), callee_inputs, zero, names],
        SemanticType::Bool,
    );
    let n = condition.u32(callee_outputs);
    let output_one = eq(&mut condition, n, 1);
    let output_ok = choose(
        &mut condition,
        output_one,
        "controlcheck-condition-type",
        "controlcheck-index-invalid",
        vec![input(0), input(1), input(2), input(3)],
        SemanticType::Bool,
    );
    let condition_ok = condition.and(input_ok, output_ok);
    let mut main = G::new("controlcheck-loop", node_args(), vec![SemanticType::Bool]);
    let op = operation(&mut main);
    let condition_at = main.advance(op, 1);
    let body_at = main.skip_blob(condition_at.clone());
    let bound = main.skip_blob(body_at.clone());
    let high = main.advance(bound.clone(), 4);
    let low = main.u32(bound);
    let high = main.u32(high);
    let zero = main.n(0);
    let low_positive = main.compare(Operation::Gt, low, zero.clone());
    let high_positive = main.compare(Operation::Gt, high, zero.clone());
    let bounded = main.logic(Operation::Or, low_positive, high_positive);
    let names = main.bool(true);
    let inputs = node_list(&mut main, 3);
    let outputs = node_list(&mut main, 4);
    let same_state = call(
        &mut main,
        "controlcheck-ports",
        vec![
            input(0),
            inputs,
            zero.clone(),
            outputs,
            zero.clone(),
            names.clone(),
        ],
        SemanticType::Bool,
    );
    let body_ok = call(
        &mut main,
        "controlcheck-target",
        vec![input(0), input(1), input(2), body_at, zero, names],
        SemanticType::Bool,
    );
    let index = call(
        &mut main,
        "validator-graph-name-index",
        vec![input(0), input(1), condition_at],
        int(),
    );
    let len = main.length(input(1));
    let found = main.compare(Operation::Lt, index.clone(), len);
    let condition_ok_main = choose(
        &mut main,
        found,
        "controlcheck-condition",
        "controlcheck-index-invalid",
        vec![input(0), input(1), input(2), index],
        SemanticType::Bool,
    );
    let valid = all(
        &mut main,
        vec![bounded, same_state, body_ok, condition_ok_main],
    );
    vec![
        invalid.finish(vec![no]),
        condition_type.finish(vec![is_bool]),
        condition.finish(vec![condition_ok]),
        main.finish(vec![valid]),
    ]
}
fn sequence(g: &mut G, at: SourceEndpoint) -> (SourceEndpoint, SourceEndpoint) {
    let array = kind(g, at.clone(), 9);
    let slice = kind(g, at.clone(), 10);
    let vector = kind(g, at.clone(), 11);
    let long = g.logic(Operation::Or, array, vector);
    let yes = g.logic(Operation::Or, long.clone(), slice);
    let long_at = g.advance(at.clone(), 9);
    let short_at = g.advance(at, 1);
    let inner = g.pick("reader-pick-offset", long, short_at, long_at);
    (yes, inner)
}
fn map_graphs() -> Vec<Graph> {
    let args = indexed_args();
    let mut bytes_proof = G::new(
        "controlcheck-map-byte-proof",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let graph = bytes_proof.index(input(1), input(3));
    let inputs = bytes_proof.field(graph, 3);
    let zero = bytes_proof.n(0);
    let ty = type_at(&mut bytes_proof, inputs, zero);
    let min_at = bytes_proof.advance(ty.clone(), 1);
    let max_at = bytes_proof.advance(ty, 17);
    let min = call(
        &mut bytes_proof,
        "reader-i128",
        vec![input(0), min_at],
        full_integer(),
    );
    let max = call(
        &mut bytes_proof,
        "reader-i128",
        vec![input(0), max_at],
        full_integer(),
    );
    let min_ok = eq(&mut bytes_proof, min, 0);
    let max_ok = eq(&mut bytes_proof, max, 255);
    let byte_exact = bytes_proof.and(min_ok, max_ok);
    let mut byte = G::new(
        "controlcheck-map-byte",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let graph = byte.index(input(1), input(3));
    let inputs = byte.field(graph, 3);
    let zero = byte.n(0);
    let ty = type_at(&mut byte, inputs, zero);
    let integer = kind(&mut byte, ty, 1);
    let byte_valid = choose(
        &mut byte,
        integer,
        "controlcheck-map-byte-proof",
        "controlcheck-index-invalid",
        vec![input(0), input(1), input(2), input(3)],
        SemanticType::Bool,
    );
    let mut element = G::new(
        "controlcheck-map-element-proof",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let node_inputs = node_list(&mut element, 3);
    let zero = element.n(0);
    let collection = type_at(&mut element, node_inputs, zero.clone());
    let (_, inner) = sequence(&mut element, collection);
    let graph = element.index(input(1), input(3));
    let body_inputs = element.field(graph, 3);
    let ty = type_at(&mut element, body_inputs, zero);
    let exact = call(
        &mut element,
        "controlcheck-type-exact",
        vec![input(0), inner, ty],
        SemanticType::Bool,
    );
    let mut element_guard = G::new(
        "controlcheck-map-element",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let list = node_list(&mut element_guard, 3);
    let zero = element_guard.n(0);
    let ty = type_at(&mut element_guard, list, zero);
    let (seq, _) = sequence(&mut element_guard, ty);
    let element_valid = choose(
        &mut element_guard,
        seq,
        "controlcheck-map-element-proof",
        "controlcheck-index-invalid",
        vec![input(0), input(1), input(2), input(3)],
        SemanticType::Bool,
    );
    let mut types = G::new(
        "controlcheck-map-types-proof",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let inputs = node_list(&mut types, 3);
    let outputs = node_list(&mut types, 4);
    let zero = types.n(0);
    let input_type = type_at(&mut types, inputs, zero.clone());
    let output_type = type_at(&mut types, outputs, zero.clone());
    let inner = types.advance(output_type, 1);
    let graph = types.index(input(1), input(3));
    let body_outputs = types.field(graph, 4);
    let body_out = type_at(&mut types, body_outputs, zero);
    let exact_out = call(
        &mut types,
        "controlcheck-type-exact",
        vec![input(0), inner, body_out],
        SemanticType::Bool,
    );
    let bytes = kind(&mut types, input_type, 8);
    let input_ok = choose(
        &mut types,
        bytes,
        "controlcheck-map-byte",
        "controlcheck-map-element",
        vec![input(0), input(1), input(2), input(3)],
        SemanticType::Bool,
    );
    let types_valid = types.and(exact_out, input_ok);
    let mut types_guard = G::new(
        "controlcheck-map-types",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let list = node_list(&mut types_guard, 4);
    let zero = types_guard.n(0);
    let ty = type_at(&mut types_guard, list, zero);
    let slice = kind(&mut types_guard, ty, 10);
    let guarded_types = choose(
        &mut types_guard,
        slice,
        "controlcheck-map-types-proof",
        "controlcheck-index-invalid",
        vec![input(0), input(1), input(2), input(3)],
        SemanticType::Bool,
    );
    let mut callee = G::new(
        "controlcheck-map-callee",
        args.clone(),
        vec![SemanticType::Bool],
    );
    let graph = callee.index(input(1), input(3));
    let inputs = callee.field(graph.clone(), 3);
    let outputs = callee.field(graph, 4);
    let ni = callee.u32(inputs);
    let no = callee.u32(outputs);
    let ni = eq(&mut callee, ni, 1);
    let no = eq(&mut callee, no, 1);
    let shape = callee.and(ni, no);
    let callee_valid = choose(
        &mut callee,
        shape,
        "controlcheck-map-types",
        "controlcheck-index-invalid",
        vec![input(0), input(1), input(2), input(3)],
        SemanticType::Bool,
    );
    let mut known = G::new(
        "controlcheck-map-known",
        node_args(),
        vec![SemanticType::Bool],
    );
    let op = operation(&mut known);
    let name = known.advance(op, 1);
    let index = call(
        &mut known,
        "validator-graph-name-index",
        vec![input(0), input(1), name],
        int(),
    );
    let len = known.length(input(1));
    let found = known.compare(Operation::Lt, index.clone(), len);
    let known_valid = choose(
        &mut known,
        found,
        "controlcheck-map-callee",
        "controlcheck-index-invalid",
        vec![input(0), input(1), input(2), index],
        SemanticType::Bool,
    );
    let mut main = G::new("controlcheck-map", node_args(), vec![SemanticType::Bool]);
    let inputs = node_list(&mut main, 3);
    let outputs = node_list(&mut main, 4);
    let ni = main.u32(inputs);
    let no = main.u32(outputs);
    let ni = eq(&mut main, ni, 1);
    let no = eq(&mut main, no, 1);
    let shape = main.and(ni, no);
    let valid = choose(
        &mut main,
        shape,
        "controlcheck-map-known",
        "controlcheck-invalid",
        vec![input(0), input(1), input(2)],
        SemanticType::Bool,
    );
    vec![
        bytes_proof.finish(vec![byte_exact]),
        byte.finish(vec![byte_valid]),
        element.finish(vec![exact]),
        element_guard.finish(vec![element_valid]),
        types.finish(vec![types_valid]),
        types_guard.finish(vec![guarded_types]),
        callee.finish(vec![callee_valid]),
        known.finish(vec![known_valid]),
        main.finish(vec![valid]),
    ]
}
pub(super) fn graphs() -> Vec<Graph> {
    let mut graphs = name_graphs();
    graphs.push(type_graph());
    graphs.extend(port_graphs());
    graphs.extend(target_graphs());
    graphs.extend(basic_graphs());
    graphs.extend(loop_graphs());
    graphs.extend(map_graphs());
    let mut invalid = G::new(
        "controlcheck-invalid",
        node_args(),
        vec![SemanticType::Bool],
    );
    let no = invalid.bool(false);
    graphs.push(invalid.finish(vec![no]));
    let mut primitive = G::new(
        "controlcheck-primitive",
        node_args(),
        vec![SemanticType::Bool],
    );
    let yes = primitive.bool(true);
    graphs.push(primitive.finish(vec![yes]));
    let mut second = G::new(
        "controlcheck-dispatch-subgraph",
        node_args(),
        vec![SemanticType::Bool],
    );
    let op = operation(&mut second);
    let tag = second.byte(op);
    let sub = eq(&mut second, tag, 7);
    let value = choose(
        &mut second,
        sub,
        "controlcheck-subgraph",
        "controlcheck-dispatch-loop",
        vec![input(0), input(1), input(2)],
        SemanticType::Bool,
    );
    graphs.push(second.finish(vec![value]));
    for (name, tag, yes, no) in [
        (
            "controlcheck-dispatch-loop",
            6,
            "controlcheck-loop",
            "controlcheck-dispatch-map",
        ),
        (
            "controlcheck-dispatch-map",
            47,
            "controlcheck-map",
            "controlcheck-dispatch-match",
        ),
        (
            "controlcheck-dispatch-match",
            5,
            "controlcheck-invalid",
            "controlcheck-primitive",
        ),
    ] {
        let mut g = G::new(name, node_args(), vec![SemanticType::Bool]);
        let op = operation(&mut g);
        let tag_value = g.byte(op);
        let test = eq(&mut g, tag_value, tag);
        let value = choose(
            &mut g,
            test,
            yes,
            no,
            vec![input(0), input(1), input(2)],
            SemanticType::Bool,
        );
        graphs.push(g.finish(vec![value]));
    }
    let mut main = G::new(
        "validator-node-control",
        node_args(),
        vec![SemanticType::Bool],
    );
    let op = operation(&mut main);
    let tag = main.byte(op);
    let select = eq(&mut main, tag, 4);
    let valid = choose(
        &mut main,
        select,
        "controlcheck-select",
        "controlcheck-dispatch-subgraph",
        vec![input(0), input(1), input(2)],
        SemanticType::Bool,
    );
    graphs.push(main.finish(vec![valid]));
    graphs
}
