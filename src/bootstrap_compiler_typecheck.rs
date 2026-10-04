//! Prefix-tree type assignment, implemented as bounded GIR state machines.
use super::*;
fn state() -> Vec<SemanticType> {
    vec![SemanticType::Bytes, int(), int(), int(), SemanticType::Bool]
}
fn outputs(
    g: &mut G,
    operation: Operation,
    args: Vec<(SourceEndpoint, SemanticType)>,
) -> Vec<SourceEndpoint> {
    let id = g.b.graph.nodes.len() as u32 + 1;
    g.op(operation, args, SemanticType::Bytes);
    g.b.graph.nodes.last_mut().unwrap().outputs = state()
        .into_iter()
        .enumerate()
        .map(|(i, ty)| port(i as u16, ty))
        .collect();
    (0..5)
        .map(|p| SourceEndpoint::NodeOutput { node: id, port: p })
        .collect()
}
fn byte_equal_graphs() -> Vec<Graph> {
    let types = vec![SemanticType::Bytes, int(), int(), int()];
    let mut loop_state = types.clone();
    loop_state.push(SemanticType::Bool);
    let mut cond = G::new(
        "validator-byte-equal-condition",
        loop_state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let test = cond.compare(Operation::Gt, input(3), zero);
    let test = cond.and(test, input(4));
    let mut body = G::new(
        "validator-byte-equal-body",
        loop_state.clone(),
        loop_state.clone(),
    );
    let a = body.byte(input(1));
    let b = body.byte(input(2));
    let equal = body.compare(Operation::Eq, a, b);
    let valid = body.and(input(4), equal);
    let a = body.advance(input(1), 1);
    let b = body.advance(input(2), 1);
    let one = body.n(1);
    let left = body.arithmetic(Operation::Sub, input(3), one);
    let mut main = G::new("validator-byte-equal", types, vec![SemanticType::Bool]);
    let yes = main.bool(true);
    let out = main.loop_node(
        "validator-byte-equal",
        loop_state,
        vec![input(0), input(1), input(2), input(3), yes],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), a, b, left, valid]),
        main.finish(vec![out[4].clone()]),
    ]
}
fn dispatch(name: &str, tags: &[usize], graphs: &mut Vec<Graph>) {
    let mut graph = G::new(name, state(), state());
    let out = if tags.len() == 1 {
        outputs(
            &mut graph,
            Operation::Subgraph(format!("validator-type-tag-{}", tags[0])),
            (0..5)
                .map(|p| (input(p), state()[p as usize].clone()))
                .collect(),
        )
    } else {
        let middle = tags.len() / 2;
        let tag = graph.byte(input(1));
        let pivot = graph.n(tags[middle] as i128);
        let test = graph.compare(Operation::Lt, tag, pivot);
        let left = format!("{name}-left");
        let right = format!("{name}-right");
        dispatch(&left, &tags[..middle], graphs);
        dispatch(&right, &tags[middle..], graphs);
        let mut args = vec![(test, SemanticType::Bool)];
        args.extend((0..5).map(|p| (input(p), state()[p as usize].clone())));
        let out = outputs(
            &mut graph,
            Operation::Select {
                when_true: left,
                when_false: right,
            },
            args,
        );
        let node = graph.b.graph.nodes.last_mut().unwrap();
        node.inputs[0].name = "selector".into();
        for (i, p) in node.inputs.iter_mut().skip(1).enumerate() {
            p.name = format!("p{i}");
        }
        out
    };
    graphs.push(graph.finish(out));
}
fn signed_byte_order() -> Graph {
    let mut g = G::new(
        "validator-signed-bytes-le",
        vec![SemanticType::Bytes, int(), int()],
        vec![SemanticType::Bool],
    );
    let length = g.n(16);
    let left = g.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (input(1), int()),
            (length.clone(), int()),
        ],
        SemanticType::Bytes,
    );
    let right = g.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (input(2), int()),
            (length, int()),
        ],
        SemanticType::Bytes,
    );
    let left = g.op(
        Operation::DecodeInteger128Le,
        vec![(left, SemanticType::Bytes)],
        full_integer(),
    );
    let right = g.op(
        Operation::DecodeInteger128Le,
        vec![(right, SemanticType::Bytes)],
        full_integer(),
    );
    let valid = g.compare(Operation::Le, left, right);
    g.finish(vec![valid])
}
pub(super) fn graphs() -> Vec<Graph> {
    let mut graphs = byte_equal_graphs();
    graphs.push(signed_byte_order());
    for tag in 0..25 {
        let mut graph = G::new(&format!("validator-type-tag-{tag}"), state(), state());
        let source_tag = graph.byte(input(1));
        let target_tag = graph.byte(input(2));
        let same = graph.compare(Operation::Eq, source_tag, target_tag);
        let mut valid = graph.and(input(4), same);
        let (source_end, target_end, pending) = if tag == 1 {
            let source_min_at = graph.advance(input(1), 1);
            let source_max_at = graph.advance(input(1), 17);
            let target_min_at = graph.advance(input(2), 1);
            let target_max_at = graph.advance(input(2), 17);
            let min_fits = call(
                &mut graph,
                "validator-signed-bytes-le",
                vec![input(0), target_min_at, source_min_at],
                SemanticType::Bool,
            );
            let max_fits = call(
                &mut graph,
                "validator-signed-bytes-le",
                vec![input(0), source_max_at, target_max_at],
                SemanticType::Bool,
            );
            valid = graph.and(valid, min_fits);
            valid = graph.and(valid, max_fits);
            let source_end = graph.advance(input(1), 33);
            let target_end = graph.advance(input(2), 33);
            let one = graph.n(1);
            let pending = graph.arithmetic(Operation::Sub, input(3), one);
            (source_end, target_end, pending)
        } else if matches!(tag,9..=11|14|15|17..=24) {
            let payload = if matches!(tag, 9 | 11) { 8 } else { 0 };
            if payload > 0 {
                let source_len_at = graph.advance(input(1), 1);
                let target_len_at = graph.advance(input(2), 1);
                let length = graph.n(8);
                let equal = call(
                    &mut graph,
                    "validator-byte-equal",
                    vec![input(0), source_len_at, target_len_at, length],
                    SemanticType::Bool,
                );
                valid = graph.and(valid, equal);
            }
            let source_end = graph.advance(input(1), payload + 1);
            let target_end = graph.advance(input(2), payload + 1);
            let pending = if tag == 15 {
                graph.advance(input(3), 1)
            } else {
                input(3)
            };
            (source_end, target_end, pending)
        } else {
            let source_end = graph.call("reader-type", input(1));
            let target_end = graph.call("reader-type", input(2));
            let source_len = graph.arithmetic(Operation::Sub, source_end.clone(), input(1));
            let target_len = graph.arithmetic(Operation::Sub, target_end.clone(), input(2));
            let lengths_equal = graph.compare(Operation::Eq, source_len.clone(), target_len);
            valid = graph.and(valid, lengths_equal);
            let equal = call(
                &mut graph,
                "validator-byte-equal",
                vec![input(0), input(1), input(2), source_len],
                SemanticType::Bool,
            );
            valid = graph.and(valid, equal);
            let one = graph.n(1);
            let pending = graph.arithmetic(Operation::Sub, input(3), one);
            (source_end, target_end, pending)
        };
        graphs.push(graph.finish(vec![input(0), source_end, target_end, pending, valid]));
    }
    dispatch(
        "validator-type-step",
        &(0..25).collect::<Vec<_>>(),
        &mut graphs,
    );
    let mut cond = G::new(
        "validator-type-assignable-condition",
        state(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let test = cond.compare(Operation::Gt, input(3), zero);
    let test = cond.and(test, input(4));
    graphs.push(cond.finish(vec![test]));
    let mut body = G::new("validator-type-assignable-body", state(), state());
    let out = outputs(
        &mut body,
        Operation::Subgraph("validator-type-step".into()),
        (0..5)
            .map(|p| (input(p), state()[p as usize].clone()))
            .collect(),
    );
    graphs.push(body.finish(out));
    let mut main = G::new(
        "validator-type-assignable",
        vec![SemanticType::Bytes, int(), int()],
        vec![SemanticType::Bool],
    );
    let one = main.n(1);
    let yes = main.bool(true);
    let out = main.loop_node(
        "validator-type-assignable",
        state(),
        vec![input(0), input(1), input(2), one, yes],
    );
    graphs.push(main.finish(vec![out[4].clone()]));
    graphs
}
