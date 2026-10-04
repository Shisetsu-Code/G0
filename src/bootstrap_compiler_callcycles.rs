//! Cached call adjacency and an iterative, colored DFS expressed in G0.
//! Framing and unique graph names are proved by the preceding parser gates.
//! Missing targets remain a sentinel and fail closed before color indexing.
use super::*;
fn seq() -> SemanticType {
    SemanticType::Slice(Box::new(int()))
}
fn rows() -> SemanticType {
    SemanticType::Slice(Box::new(seq()))
}
fn args(g: &G, values: Vec<SourceEndpoint>) -> Vec<(SourceEndpoint, SemanticType)> {
    values
        .into_iter()
        .map(|v| {
            let mut t = g.ty(&v);
            if matches!(&t,SemanticType::Integer(r)if r.min>=0&&r.max<=1_i128<<48) {
                t = int();
            }
            (v, t)
        })
        .collect()
}
fn sub(g: &mut G, name: &str, values: Vec<SourceEndpoint>, out: SemanticType) -> SourceEndpoint {
    let values = args(g, values);
    g.op(Operation::Subgraph(name.into()), values, out)
}
fn select(
    g: &mut G,
    test: SourceEndpoint,
    yes: &str,
    no: &str,
    values: Vec<SourceEndpoint>,
    outputs: Vec<SemanticType>,
) -> Vec<SourceEndpoint> {
    let mut values = args(g, values);
    values.insert(0, (test, SemanticType::Bool));
    let id = g.b.graph.nodes.len() as u32 + 1;
    g.op(
        Operation::Select {
            when_true: yes.into(),
            when_false: no.into(),
        },
        values,
        outputs[0].clone(),
    );
    let n = g.b.graph.nodes.last_mut().unwrap();
    n.inputs[0].name = "selector".into();
    for (i, p) in n.inputs[1..].iter_mut().enumerate() {
        p.name = format!("p{i}");
    }
    n.outputs = outputs
        .into_iter()
        .enumerate()
        .map(|(i, t)| port(i as u16, t))
        .collect();
    (0..n.outputs.len())
        .map(|p| SourceEndpoint::NodeOutput {
            node: id,
            port: p as u16,
        })
        .collect()
}
fn op(
    g: &mut G,
    operation: Operation,
    values: Vec<SourceEndpoint>,
    ty: SemanticType,
) -> SourceEndpoint {
    let values = args(g, values);
    g.op(operation, values, ty)
}
fn array(g: &mut G, values: Vec<SourceEndpoint>, ty: SemanticType) -> SourceEndpoint {
    op(g, Operation::MakeArray, values, ty)
}
fn append(g: &mut G, a: SourceEndpoint, b: SourceEndpoint, ty: SemanticType) -> SourceEndpoint {
    let one = array(g, vec![b], ty.clone());
    op(g, Operation::ArrayConcat, vec![a, one], ty)
}
fn empty(g: &mut G, ty: SemanticType) -> SourceEndpoint {
    array(g, vec![], ty)
}
fn eq(g: &mut G, a: SourceEndpoint, n: i128) -> SourceEndpoint {
    let b = g.n(n);
    g.compare(Operation::Eq, a, b)
}
fn length(g: &mut G, a: SourceEndpoint) -> SourceEndpoint {
    let n = g.length(a);
    let ty = g.ty(&n);
    g.op(Operation::ConvertChecked, vec![(n, ty)], int())
}
fn color_at(g: &mut G, at: SourceEndpoint) -> SourceEndpoint {
    let byte = SemanticType::Integer(IntegerType { min: 0, max: 255 });
    let option = SemanticType::Option(Box::new(byte.clone()));
    let value = g.op(
        Operation::Index,
        vec![(input(1), SemanticType::Bytes), (at, int())],
        option.clone(),
    );
    let zero = g.n(0);
    g.op(
        Operation::UnwrapOr,
        vec![(value, option), (zero, byte.clone())],
        byte,
    )
}
fn resolve(g: &mut G, at: SourceEndpoint) -> SourceEndpoint {
    sub(
        g,
        "validator-graph-name-index",
        vec![input(0), input(1), at],
        int(),
    )
}

fn reference_graphs() -> Vec<Graph> {
    let inputs = vec![SemanticType::Bytes, rows(), int()];
    let mut graphs = vec![];
    let mut none = G::new("callcycles-ref-none", inputs.clone(), vec![seq()]);
    let e = empty(&mut none, seq());
    graphs.push(none.finish(vec![e]));
    let mut one = G::new("callcycles-ref-one", inputs.clone(), vec![seq()]);
    let at = one.advance(input(2), 1);
    let target = resolve(&mut one, at);
    let a = array(&mut one, vec![target], seq());
    graphs.push(one.finish(vec![a]));
    let mut two = G::new("callcycles-ref-two", inputs.clone(), vec![seq()]);
    let at = two.advance(input(2), 1);
    let second = two.skip_blob(at.clone());
    let a = resolve(&mut two, at);
    let b = resolve(&mut two, second);
    let a = array(&mut two, vec![a, b], seq());
    graphs.push(two.finish(vec![a]));
    let state = vec![SemanticType::Bytes, rows(), int(), int(), seq()];
    let mut c = G::new(
        "callcycles-match-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = c.n(0);
    let more = c.compare(Operation::Gt, input(3), zero);
    let mut b = G::new("callcycles-match-body", state.clone(), state.clone());
    let name = b.skip_blob(input(2));
    let next = b.skip_blob(name.clone());
    let target = resolve(&mut b, name);
    let body_refs = append(&mut b, input(4), target, seq());
    let one = b.n(1);
    let left = b.arithmetic(Operation::Sub, input(3), one);
    let mut default = G::new(
        "callcycles-ref-default",
        vec![SemanticType::Bytes, rows(), int(), seq()],
        vec![seq()],
    );
    let target = resolve(&mut default, input(2));
    let refs_default = append(&mut default, input(3), target, seq());
    let mut m = G::new("callcycles-ref-match", inputs.clone(), vec![seq()]);
    let count_at = m.advance(input(2), 1);
    let count = m.u32(count_at);
    let at = m.advance(input(2), 5);
    let e = empty(&mut m, seq());
    let out = m.loop_node(
        "callcycles-match",
        state,
        vec![input(0), input(1), at, count, e],
    );
    let refs = sub(
        &mut m,
        "callcycles-ref-default",
        vec![input(0), input(1), out[2].clone(), out[4].clone()],
        seq(),
    );
    graphs.extend([
        c.finish(vec![more]),
        b.finish(vec![input(0), input(1), next, left, body_refs]),
        default.finish(vec![refs_default]),
        m.finish(vec![refs]),
    ]);
    for (name, tags, yes, no) in [
        (
            "callcycles-ref-single",
            vec![7, 47],
            "callcycles-ref-one",
            "callcycles-ref-double",
        ),
        (
            "callcycles-ref-double",
            vec![4, 6],
            "callcycles-ref-two",
            "callcycles-ref-match-test",
        ),
        (
            "callcycles-ref-match-test",
            vec![5],
            "callcycles-ref-match",
            "callcycles-ref-none",
        ),
    ] {
        let mut g = G::new(name, inputs.clone(), vec![seq()]);
        let tag = g.byte(input(2));
        let mut tests = tags
            .into_iter()
            .map(|n| eq(&mut g, tag.clone(), n))
            .collect::<Vec<_>>();
        let test = tests
            .drain(..)
            .reduce(|a, b| g.logic(Operation::Or, a, b))
            .unwrap();
        let out = select(
            &mut g,
            test,
            yes,
            no,
            vec![input(0), input(1), input(2)],
            vec![seq()],
        )[0]
        .clone();
        graphs.push(g.finish(vec![out]));
    }
    graphs
}

fn cache_graphs() -> Vec<Graph> {
    let ns = vec![SemanticType::Bytes, rows(), rows(), int(), seq()];
    let mut nc = G::new(
        "callcycles-nodes-condition",
        ns.clone(),
        vec![SemanticType::Bool],
    );
    let len = nc.length(input(2));
    let more = nc.compare(Operation::Lt, input(3), len);
    let mut nb = G::new("callcycles-nodes-body", ns.clone(), ns.clone());
    let row = nb.index(input(2), input(3));
    let at = nb.field(row, 2);
    let targets = sub(
        &mut nb,
        "callcycles-ref-single",
        vec![input(0), input(1), at],
        seq(),
    );
    let refs = op(
        &mut nb,
        Operation::ArrayConcat,
        vec![input(4), targets],
        seq(),
    );
    let next = nb.advance(input(3), 1);
    let mut graph = G::new(
        "callcycles-graph",
        vec![SemanticType::Bytes, rows(), int()],
        vec![seq()],
    );
    let nodes = sub(
        &mut graph,
        "reader-graph-ast",
        vec![input(0), input(2)],
        rows(),
    );
    let zero = graph.n(0);
    let e = empty(&mut graph, seq());
    let out = graph.loop_node(
        "callcycles-nodes",
        ns,
        vec![input(0), input(1), nodes, zero, e],
    );
    let ps = vec![SemanticType::Bytes, rows(), int(), rows()];
    let mut pc = G::new(
        "callcycles-cache-condition",
        ps.clone(),
        vec![SemanticType::Bool],
    );
    let len = pc.length(input(1));
    let test = pc.compare(Operation::Lt, input(2), len);
    let mut pb = G::new("callcycles-cache-body", ps.clone(), ps.clone());
    let row = pb.index(input(1), input(2));
    let at = pb.field(row, 0);
    let targets = sub(
        &mut pb,
        "callcycles-graph",
        vec![input(0), input(1), at],
        seq(),
    );
    let cache = append(&mut pb, input(3), targets, rows());
    let next_g = pb.advance(input(2), 1);
    let mut main = G::new(
        "callcycles-cache",
        vec![SemanticType::Bytes, rows()],
        vec![rows()],
    );
    let zero = main.n(0);
    let e = empty(&mut main, rows());
    let out_g = main.loop_node("callcycles-cache", ps, vec![input(0), input(1), zero, e]);
    vec![
        nc.finish(vec![more]),
        nb.finish(vec![input(0), input(1), input(2), next, refs]),
        graph.finish(vec![out[4].clone()]),
        pc.finish(vec![test]),
        pb.finish(vec![input(0), input(1), next_g, cache]),
        main.finish(vec![out_g[3].clone()]),
    ]
}

fn color_graphs() -> Vec<Graph> {
    let byte = SemanticType::Integer(IntegerType { min: 0, max: 255 });
    let mut z = G::new("callcycles-zero-byte", vec![int()], vec![byte.clone()]);
    let zero = z.n(0);
    let mut colors = G::new("callcycles-colors", vec![int()], vec![SemanticType::Bytes]);
    let range = op(&mut colors, Operation::Range, vec![input(0)], seq());
    let bytes = op(
        &mut colors,
        Operation::Map {
            body: "callcycles-zero-byte".into(),
        },
        vec![range],
        SemanticType::Slice(Box::new(byte)),
    );
    let colors_out = op(
        &mut colors,
        Operation::BytesFromArray,
        vec![bytes],
        SemanticType::Bytes,
    );
    let mut graphs = vec![z.finish(vec![zero]), colors.finish(vec![colors_out])];
    for (name, value) in [("callcycles-gray", 1), ("callcycles-black", 2)] {
        let mut g = G::new(
            name,
            vec![SemanticType::Bytes, int()],
            vec![SemanticType::Bytes],
        );
        let zero = g.n(0);
        let before = op(
            &mut g,
            Operation::BytesSlice,
            vec![input(0), zero, input(1)],
            SemanticType::Bytes,
        );
        let next = g.advance(input(1), 1);
        let len = g.length(input(0));
        let remaining = g.arithmetic(Operation::Sub, len, next.clone());
        let after = op(
            &mut g,
            Operation::BytesSlice,
            vec![input(0), next, remaining],
            SemanticType::Bytes,
        );
        let byte = g.op(
            Operation::Const(Literal::Bytes(vec![value])),
            vec![],
            SemanticType::Bytes,
        );
        let first = op(
            &mut g,
            Operation::BytesConcat,
            vec![before, byte],
            SemanticType::Bytes,
        );
        let out = op(
            &mut g,
            Operation::BytesConcat,
            vec![first, after],
            SemanticType::Bytes,
        );
        graphs.push(g.finish(vec![out]));
    }
    graphs
}

// State: cached adjacency, colors, immutable explicit stack frames,
// top frame index, next root, stack active, valid. Parent indices are +1;
// zero is the root sentinel. Cursor updates append a replacement frame.
fn dfs_state() -> Vec<SemanticType> {
    vec![
        rows(),
        SemanticType::Bytes,
        rows(),
        int(),
        int(),
        SemanticType::Bool,
        SemanticType::Bool,
    ]
}
fn state_args() -> Vec<SourceEndpoint> {
    (0..7).map(input).collect()
}
fn frame(
    g: &mut G,
    vertex: SourceEndpoint,
    parent: SourceEndpoint,
    cursor: SourceEndpoint,
) -> SourceEndpoint {
    array(g, vec![vertex, parent, cursor], seq())
}
fn top_frame(g: &mut G) -> (SourceEndpoint, SourceEndpoint, SourceEndpoint) {
    let row = g.index(input(2), input(3));
    let v = g.field(row.clone(), 0);
    let p = g.field(row.clone(), 1);
    let c = g.field(row, 2);
    (v, p, c)
}
fn dfs_graphs() -> Vec<Graph> {
    let s = dfs_state();
    let mut graphs = vec![];
    let mut condition = G::new(
        "callcycles-dfs-condition",
        s.clone(),
        vec![SemanticType::Bool],
    );
    let len = condition.length(input(0));
    let roots = condition.compare(Operation::Lt, input(4), len);
    let more = condition.logic(Operation::Or, roots, input(5));
    let more = condition.and(more, input(6));
    graphs.push(condition.finish(vec![more]));
    let mut invalid = G::new("callcycles-dfs-invalid", s.clone(), s.clone());
    let no = invalid.bool(false);
    let mut invalid_out = state_args();
    invalid_out[6] = no;
    graphs.push(invalid.finish(invalid_out));
    let mut skip = G::new("callcycles-dfs-root-skip", s.clone(), s.clone());
    let next = skip.advance(input(4), 1);
    let mut out = state_args();
    out[4] = next;
    graphs.push(skip.finish(out));
    let mut root = G::new("callcycles-dfs-root-push", s.clone(), s.clone());
    let zero = root.n(0);
    let f = frame(&mut root, input(4), zero.clone(), zero);
    let top = length(&mut root, input(2));
    let frames = append(&mut root, input(2), f, rows());
    let colors = sub(
        &mut root,
        "callcycles-gray",
        vec![input(1), input(4)],
        SemanticType::Bytes,
    );
    let next = root.advance(input(4), 1);
    let yes = root.bool(true);
    graphs.push(root.finish(vec![input(0), colors, frames, top, next, yes, input(6)]));
    let mut idle = G::new("callcycles-dfs-idle", s.clone(), s.clone());
    let color = color_at(&mut idle, input(4));
    let white = eq(&mut idle, color, 0);
    let out = select(
        &mut idle,
        white,
        "callcycles-dfs-root-push",
        "callcycles-dfs-root-skip",
        state_args(),
        s.clone(),
    );
    graphs.push(idle.finish(out));
    let mut done = G::new("callcycles-dfs-root-done", s.clone(), s.clone());
    let (v, _, _) = top_frame(&mut done);
    let colors = sub(
        &mut done,
        "callcycles-black",
        vec![input(1), v],
        SemanticType::Bytes,
    );
    let no = done.bool(false);
    graphs.push(done.finish(vec![
        input(0),
        colors,
        input(2),
        input(3),
        input(4),
        no,
        input(6),
    ]));
    let mut pop = G::new("callcycles-dfs-pop", s.clone(), s.clone());
    let (v, p, _) = top_frame(&mut pop);
    let colors = sub(
        &mut pop,
        "callcycles-black",
        vec![input(1), v],
        SemanticType::Bytes,
    );
    let one = pop.n(1);
    let top = pop.arithmetic(Operation::Sub, p, one);
    graphs.push(pop.finish(vec![
        input(0),
        colors,
        input(2),
        top,
        input(4),
        input(5),
        input(6),
    ]));
    let mut exhausted = G::new("callcycles-dfs-exhausted", s.clone(), s.clone());
    let (_, p, _) = top_frame(&mut exhausted);
    let root = eq(&mut exhausted, p, 0);
    let out = select(
        &mut exhausted,
        root,
        "callcycles-dfs-root-done",
        "callcycles-dfs-pop",
        state_args(),
        s.clone(),
    );
    graphs.push(exhausted.finish(out));
    // Every edge advances its parent's cursor exactly once.
    let mut next_edge = G::new("callcycles-dfs-next", s.clone(), s.clone());
    let (v, p, c) = top_frame(&mut next_edge);
    let c = next_edge.advance(c, 1);
    let f = frame(&mut next_edge, v, p, c);
    let top = length(&mut next_edge, input(2));
    let frames = append(&mut next_edge, input(2), f, rows());
    graphs.push(next_edge.finish(vec![
        input(0),
        input(1),
        frames,
        top,
        input(4),
        input(5),
        input(6),
    ]));
    let extended = {
        let mut x = s.clone();
        x.push(int());
        x
    };
    let mut child = G::new("callcycles-dfs-child", extended.clone(), s.clone());
    let parent = child.advance(input(3), 1);
    let zero = child.n(0);
    let f = frame(&mut child, input(7), parent, zero);
    let top = length(&mut child, input(2));
    let frames = append(&mut child, input(2), f, rows());
    let colors = sub(
        &mut child,
        "callcycles-gray",
        vec![input(1), input(7)],
        SemanticType::Bytes,
    );
    graphs.push(child.finish(vec![
        input(0),
        colors,
        frames,
        top,
        input(4),
        input(5),
        input(6),
    ]));
    let mut black = G::new("callcycles-dfs-black-edge", extended.clone(), s.clone());
    let (v, p, c) = top_frame(&mut black);
    let c = black.advance(c, 1);
    let f = frame(&mut black, v, p, c);
    let top = length(&mut black, input(2));
    let frames = append(&mut black, input(2), f, rows());
    graphs.push(black.finish(vec![
        input(0),
        input(1),
        frames,
        top,
        input(4),
        input(5),
        input(6),
    ]));
    let mut gray = G::new("callcycles-dfs-gray-edge", extended.clone(), s.clone());
    let no = gray.bool(false);
    let mut out = state_args();
    out[6] = no;
    graphs.push(gray.finish(out));
    let mut white = G::new("callcycles-dfs-white-edge", extended.clone(), s.clone());
    let (v, p, c) = top_frame(&mut white);
    let c = white.advance(c, 1);
    let f = frame(&mut white, v, p, c);
    let top = length(&mut white, input(2));
    let frames = append(&mut white, input(2), f, rows());
    let yes = white.bool(true);
    let out = select(
        &mut white,
        yes,
        "callcycles-dfs-child",
        "callcycles-dfs-child",
        vec![
            input(0),
            input(1),
            frames,
            top,
            input(4),
            input(5),
            input(6),
            input(7),
        ],
        s.clone(),
    );
    graphs.push(white.finish(out));
    let mut visited = G::new("callcycles-dfs-visited-edge", extended.clone(), s.clone());
    let color = color_at(&mut visited, input(7));
    let is_gray = eq(&mut visited, color, 1);
    let mut a = state_args();
    a.push(input(7));
    let out = select(
        &mut visited,
        is_gray,
        "callcycles-dfs-gray-edge",
        "callcycles-dfs-black-edge",
        a,
        s.clone(),
    );
    graphs.push(visited.finish(out));
    let mut known = G::new("callcycles-dfs-known-edge", extended.clone(), s.clone());
    let color = color_at(&mut known, input(7));
    let white_color = eq(&mut known, color, 0);
    let mut a = state_args();
    a.push(input(7));
    let out = select(
        &mut known,
        white_color,
        "callcycles-dfs-white-edge",
        "callcycles-dfs-visited-edge",
        a,
        s.clone(),
    );
    graphs.push(known.finish(out));
    let mut missing = G::new("callcycles-dfs-missing-edge", extended.clone(), s.clone());
    let no = missing.bool(false);
    let mut out = state_args();
    out[6] = no;
    graphs.push(missing.finish(out));
    let mut edge = G::new("callcycles-dfs-edge", s.clone(), s.clone());
    let (v, _, c) = top_frame(&mut edge);
    let row = edge.index(input(0), v);
    let target = edge.index(row, c);
    let len = edge.length(input(0));
    let exists = edge.compare(Operation::Lt, target.clone(), len);
    let mut a = state_args();
    a.push(target);
    let out = select(
        &mut edge,
        exists,
        "callcycles-dfs-known-edge",
        "callcycles-dfs-missing-edge",
        a,
        s.clone(),
    );
    graphs.push(edge.finish(out));
    let mut active = G::new("callcycles-dfs-active", s.clone(), s.clone());
    let (v, _, c) = top_frame(&mut active);
    let row = active.index(input(0), v);
    let len = active.length(row);
    let more = active.compare(Operation::Lt, c, len);
    let out = select(
        &mut active,
        more,
        "callcycles-dfs-edge",
        "callcycles-dfs-exhausted",
        state_args(),
        s.clone(),
    );
    graphs.push(active.finish(out));
    let mut body = G::new("callcycles-dfs-body", s.clone(), s.clone());
    let out = select(
        &mut body,
        input(5),
        "callcycles-dfs-active",
        "callcycles-dfs-idle",
        state_args(),
        s,
    );
    graphs.push(body.finish(out));
    graphs
}
pub(super) fn graphs() -> Vec<Graph> {
    let mut graphs = reference_graphs();
    graphs.extend(cache_graphs());
    graphs.extend(color_graphs());
    graphs.extend(dfs_graphs());
    let mut g = G::new(
        "validator-program-callcycles",
        vec![SemanticType::Bytes],
        vec![SemanticType::Bool],
    );
    let descriptors = sub(&mut g, "reader-program-ast", vec![input(0)], rows());
    let adj = sub(
        &mut g,
        "callcycles-cache",
        vec![input(0), descriptors],
        rows(),
    );
    let len = length(&mut g, adj.clone());
    let colors = sub(&mut g, "callcycles-colors", vec![len], SemanticType::Bytes);
    let frames = empty(&mut g, rows());
    let zero = g.n(0);
    let no = g.bool(false);
    let yes = g.bool(true);
    let out = g.loop_node(
        "callcycles-dfs",
        dfs_state(),
        vec![adj, colors, frames, zero.clone(), zero, no, yes],
    );
    graphs.push(g.finish(vec![out[6].clone()]));
    let fast = super::control_fast::variants(
        &graphs,
        &[("validator-graph-name-index", "validator-name-index-fast")],
    );
    graphs.extend(fast);
    graphs
}
