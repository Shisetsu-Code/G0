//! Canonical UTF-8 name ordering and bounded binary lookup expressed in G0.
//! Lookup assumes parsed UTF-8 names and strict ordering proved by the companion gate.
//! Comparisons return 0 for equal, 1 for less, and 2 for greater.
use super::*;

fn rows() -> SemanticType {
    SemanticType::Slice(Box::new(SemanticType::Slice(Box::new(int()))))
}
fn call(g: &mut G, name: &str, values: Vec<SourceEndpoint>, ty: SemanticType) -> SourceEndpoint {
    let args = values
        .into_iter()
        .map(|v| {
            let ty = match g.ty(&v) {
                SemanticType::Integer(_) => int(),
                other => other,
            };
            (v, ty)
        })
        .collect();
    g.op(Operation::Subgraph(name.into()), args, ty)
}
fn pick(
    g: &mut G,
    graphs: &mut Vec<Graph>,
    name: &str,
    test: SourceEndpoint,
    a: SourceEndpoint,
    b: SourceEndpoint,
) -> SourceEndpoint {
    let ty = match g.ty(&a) {
        SemanticType::Integer(_) => int(),
        other => other,
    };
    graphs.push(
        G::new(
            &format!("{name}-first"),
            vec![ty.clone(), ty.clone()],
            vec![ty.clone()],
        )
        .finish(vec![input(0)]),
    );
    graphs.push(
        G::new(
            &format!("{name}-second"),
            vec![ty.clone(), ty.clone()],
            vec![ty],
        )
        .finish(vec![input(1)]),
    );
    g.pick(name, test, a, b)
}
fn byte(g: &mut G, bytes: SourceEndpoint, at: SourceEndpoint) -> SourceEndpoint {
    let ty = SemanticType::Integer(IntegerType { min: 0, max: 255 });
    let optional = SemanticType::Option(Box::new(ty.clone()));
    let value = g.op(
        Operation::Index,
        vec![(bytes, SemanticType::Bytes), (at, int())],
        optional.clone(),
    );
    let zero = g.op(Operation::Const(Literal::Integer(0)), vec![], ty.clone());
    g.op(
        Operation::UnwrapOr,
        vec![(value, optional), (zero, ty.clone())],
        ty,
    )
}

pub(super) fn graphs() -> Vec<Graph> {
    let mut graphs = Vec::new();
    // Compact the two names before looping: retaining the whole program as a
    // loop value would repeatedly charge its logical byte size.
    let state = vec![SemanticType::Bytes, int(), int(), int(), int()];
    let mut condition = G::new(
        "validator-name-lex-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let left_more = condition.compare(Operation::Lt, input(3), input(1));
    let right_more = condition.compare(Operation::Lt, input(3), input(2));
    let more = condition.and(left_more, right_more);
    let zero = condition.n(0);
    let equal = condition.compare(Operation::Eq, input(4), zero);
    let more = condition.and(more, equal);
    graphs.push(condition.finish(vec![more]));
    let mut body = G::new("validator-name-lex-body", state.clone(), state.clone());
    let left_byte = byte(&mut body, input(0), input(3));
    let right_at = body.arithmetic(Operation::Add, input(1), input(3));
    let right_byte = byte(&mut body, input(0), right_at);
    let smaller = body.compare(Operation::Lt, left_byte.clone(), right_byte.clone());
    let equal = body.compare(Operation::Eq, left_byte, right_byte);
    let plus = body.n(2);
    let minus = body.n(1);
    let unequal = pick(
        &mut body,
        &mut graphs,
        "validator-name-byte-sign",
        smaller,
        plus,
        minus,
    );
    let zero = body.n(0);
    let sign = pick(
        &mut body,
        &mut graphs,
        "validator-name-byte-equal",
        equal,
        unequal,
        zero,
    );
    let one = body.n(1);
    let next = body.arithmetic(Operation::Add, input(3), one);
    graphs.push(body.finish(vec![input(0), input(1), input(2), next, sign]));
    let mut compare = G::new(
        "validator-name-compare-fast",
        vec![SemanticType::Bytes, int(), int()],
        vec![int()],
    );
    let left_len = compare.u32(input(1));
    let right_len = compare.u32(input(2));
    let left_at = compare.advance(input(1), 4);
    let right_at = compare.advance(input(2), 4);
    let left = compare.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (left_at, int()),
            (left_len.clone(), int()),
        ],
        SemanticType::Bytes,
    );
    let right = compare.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (right_at, int()),
            (right_len.clone(), int()),
        ],
        SemanticType::Bytes,
    );
    let compact = compare.op(
        Operation::BytesConcat,
        vec![(left, SemanticType::Bytes), (right, SemanticType::Bytes)],
        SemanticType::Bytes,
    );
    let zero = compare.n(0);
    let out = compare.loop_node(
        "validator-name-lex",
        state,
        vec![
            compact,
            left_len.clone(),
            right_len.clone(),
            zero.clone(),
            zero.clone(),
        ],
    );
    let shorter = compare.compare(Operation::Lt, left_len.clone(), right_len.clone());
    let same_len = compare.compare(Operation::Eq, left_len, right_len);
    let plus = compare.n(2);
    let minus = compare.n(1);
    let length_sign = pick(
        &mut compare,
        &mut graphs,
        "validator-name-length-sign",
        shorter,
        plus,
        minus,
    );
    let prefix_sign = pick(
        &mut compare,
        &mut graphs,
        "validator-name-length-equal",
        same_len,
        length_sign,
        zero.clone(),
    );
    let same_prefix = compare.compare(Operation::Eq, out[4].clone(), zero);
    let sign = pick(
        &mut compare,
        &mut graphs,
        "validator-name-prefix-sign",
        same_prefix,
        out[4].clone(),
        prefix_sign,
    );
    graphs.push(compare.finish(vec![sign]));

    let state = vec![SemanticType::Bytes, rows(), int(), SemanticType::Bool];
    let mut condition = G::new(
        "validator-name-order-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = condition.length(input(1));
    let more = condition.compare(Operation::Lt, input(2), len);
    let more = condition.and(more, input(3));
    graphs.push(condition.finish(vec![more]));
    let mut body = G::new("validator-name-order-body", state.clone(), state.clone());
    let one = body.n(1);
    let prev = body.arithmetic(Operation::Sub, input(2), one.clone());
    let left = body.index(input(1), prev);
    let left = body.field(left, 2);
    let right = body.index(input(1), input(2));
    let right = body.field(right, 2);
    let right_len = body.u32(right.clone());
    let zero = body.n(0);
    let nonempty = body.compare(Operation::Gt, right_len, zero.clone());
    let sign = call(
        &mut body,
        "validator-name-compare-fast",
        vec![input(0), left, right],
        int(),
    );
    let one_sign = body.n(1);
    let ordered = body.compare(Operation::Eq, sign, one_sign);
    let valid = body.and(ordered, nonempty);
    let next = body.arithmetic(Operation::Add, input(2), one);
    graphs.push(body.finish(vec![input(0), input(1), next, valid]));
    let mut scan = G::new(
        "validator-name-order-scan",
        vec![SemanticType::Bytes, rows()],
        vec![SemanticType::Bool],
    );
    let first = scan.field(input(1), 0);
    let at = scan.field(first, 2);
    let first_len = scan.u32(at);
    let zero = scan.n(0);
    let valid = scan.compare(Operation::Gt, first_len, zero);
    let one = scan.n(1);
    let out = scan.loop_node(
        "validator-name-order",
        state,
        vec![input(0), input(1), one, valid],
    );
    graphs.push(scan.finish(vec![out[3].clone()]));
    let mut empty = G::new(
        "validator-name-order-empty",
        vec![SemanticType::Bytes, rows()],
        vec![SemanticType::Bool],
    );
    let no = empty.bool(false);
    graphs.push(empty.finish(vec![no]));
    let mut main = G::new(
        "validator-canonical-names",
        vec![SemanticType::Bytes, rows()],
        vec![SemanticType::Bool],
    );
    let len = main.length(input(1));
    let zero = main.n(0);
    let nonempty = main.compare(Operation::Gt, len, zero);
    let out = main.op(
        Operation::Select {
            when_true: "validator-name-order-scan".into(),
            when_false: "validator-name-order-empty".into(),
        },
        vec![
            (nonempty, SemanticType::Bool),
            (input(0), SemanticType::Bytes),
            (input(1), rows()),
        ],
        SemanticType::Bool,
    );
    let node = main.b.graph.nodes.last_mut().unwrap();
    node.inputs[0].name = "selector".into();
    node.inputs[1].name = "p0".into();
    node.inputs[2].name = "p1".into();
    graphs.push(main.finish(vec![out]));

    // bytes, rows, target-name offset, low, exclusive high, found/sentinel.
    let state = vec![SemanticType::Bytes, rows(), int(), int(), int(), int()];
    let mut condition = G::new(
        "validator-name-binary-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let more = condition.compare(Operation::Lt, input(3), input(4));
    let len = condition.length(input(1));
    let missing = condition.compare(Operation::Eq, input(5), len);
    let more = condition.and(more, missing);
    graphs.push(condition.finish(vec![more]));
    let mut body = G::new("validator-name-binary-body", state.clone(), state.clone());
    let total = body.arithmetic(Operation::Add, input(3), input(4));
    let two = body.n(2);
    let middle = body.op(Operation::Div, vec![(total, int()), (two, int())], int());
    let descriptor = body.index(input(1), middle.clone());
    let at = body.field(descriptor, 2);
    let sign = call(
        &mut body,
        "validator-name-compare-fast",
        vec![input(0), input(2), at],
        int(),
    );
    let zero = body.n(0);
    let one_sign = body.n(1);
    let two_sign = body.n(2);
    let smaller = body.compare(Operation::Eq, sign.clone(), one_sign);
    let bigger = body.compare(Operation::Eq, sign.clone(), two_sign);
    let equal = body.compare(Operation::Eq, sign, zero);
    let one = body.n(1);
    let after_middle = body.arithmetic(Operation::Add, middle.clone(), one);
    let low = pick(
        &mut body,
        &mut graphs,
        "validator-name-binary-low",
        bigger,
        input(3),
        after_middle,
    );
    let high = pick(
        &mut body,
        &mut graphs,
        "validator-name-binary-high",
        smaller,
        input(4),
        middle.clone(),
    );
    let found = pick(
        &mut body,
        &mut graphs,
        "validator-name-binary-found",
        equal,
        input(5),
        middle,
    );
    graphs.push(body.finish(vec![input(0), input(1), input(2), low, high, found]));
    let mut main = G::new(
        "validator-name-index-fast",
        vec![SemanticType::Bytes, rows(), int()],
        vec![int()],
    );
    let zero = main.n(0);
    let count = main.length(input(1));
    let count_ty = main.ty(&count);
    let count = main.op(Operation::ConvertChecked, vec![(count, count_ty)], int());
    let out = main.loop_node(
        "validator-name-binary",
        state,
        vec![input(0), input(1), input(2), zero, count.clone(), count],
    );
    graphs.push(main.finish(vec![out[5].clone()]));
    graphs
}
