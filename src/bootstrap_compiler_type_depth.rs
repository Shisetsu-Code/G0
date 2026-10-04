//! A 129-byte explicit subtree stack enforces depth without limiting tree width.
use super::*;
fn byte(g: &mut G, bytes: SourceEndpoint, at: SourceEndpoint) -> SourceEndpoint {
    let ty = SemanticType::Integer(IntegerType { min: 0, max: 255 });
    let option = SemanticType::Option(Box::new(ty));
    let value = g.op(
        Operation::Index,
        vec![(bytes, SemanticType::Bytes), (at, int())],
        option.clone(),
    );
    let zero = g.n(0);
    g.op(
        Operation::UnwrapOr,
        vec![(value, option), (zero, int())],
        int(),
    )
}
fn choose_bytes(
    g: &mut G,
    test: SourceEndpoint,
    left: SourceEndpoint,
    right: SourceEndpoint,
) -> SourceEndpoint {
    let out = g.op(
        Operation::Select {
            when_true: "type-depth-bytes-right".into(),
            when_false: "type-depth-bytes-left".into(),
        },
        vec![
            (test, SemanticType::Bool),
            (left, SemanticType::Bytes),
            (right, SemanticType::Bytes),
        ],
        SemanticType::Bytes,
    );
    let node = g.b.graph.nodes.last_mut().unwrap();
    node.inputs[0].name = "selector".into();
    node.inputs[1].name = "p0".into();
    node.inputs[2].name = "p1".into();
    out
}
pub(super) fn graphs() -> Vec<Graph> {
    let mut set = G::new(
        "type-depth-stack-set",
        vec![SemanticType::Bytes, int(), int()],
        vec![SemanticType::Bytes],
    );
    let zero = set.n(0);
    let before = set.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (zero, int()),
            (input(1), int()),
        ],
        SemanticType::Bytes,
    );
    let next = set.advance(input(1), 1);
    let size = set.n(129);
    let size = set.arithmetic(Operation::Sub, size, next.clone());
    let after = set.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (next, int()),
            (size, int()),
        ],
        SemanticType::Bytes,
    );
    let byte_ty = SemanticType::Integer(IntegerType { min: 0, max: 255 });
    let value = set.op(
        Operation::ConvertChecked,
        vec![(input(2), int())],
        byte_ty.clone(),
    );
    let array = set.op(
        Operation::MakeArray,
        vec![(value, byte_ty.clone())],
        SemanticType::Slice(Box::new(byte_ty.clone())),
    );
    let value = set.op(
        Operation::BytesFromArray,
        vec![(array, SemanticType::Slice(Box::new(byte_ty)))],
        SemanticType::Bytes,
    );
    let prefix = set.op(
        Operation::BytesConcat,
        vec![(before, SemanticType::Bytes), (value, SemanticType::Bytes)],
        SemanticType::Bytes,
    );
    let stack_set = set.op(
        Operation::BytesConcat,
        vec![(prefix, SemanticType::Bytes), (after, SemanticType::Bytes)],
        SemanticType::Bytes,
    );
    let ascend_state = vec![SemanticType::Bytes, int()];
    let mut ac = G::new(
        "type-depth-ascend-condition",
        ascend_state.clone(),
        vec![SemanticType::Bool],
    );
    let remaining = byte(&mut ac, input(0), input(1));
    let zero = ac.n(0);
    let exhausted = ac.compare(Operation::Eq, remaining, zero.clone());
    let above = ac.compare(Operation::Gt, input(1), zero);
    let ascend_more = ac.and(exhausted, above);
    let mut ab = G::new(
        "type-depth-ascend-body",
        ascend_state.clone(),
        ascend_state.clone(),
    );
    let one = ab.n(1);
    let parent = ab.arithmetic(Operation::Sub, input(1), one);
    let left = G::new(
        "type-depth-bytes-left",
        vec![SemanticType::Bytes, SemanticType::Bytes],
        vec![SemanticType::Bytes],
    );
    let right = G::new(
        "type-depth-bytes-right",
        vec![SemanticType::Bytes, SemanticType::Bytes],
        vec![SemanticType::Bytes],
    );
    let state = vec![
        SemanticType::Bytes,
        rows(),
        int(),
        int(),
        int(),
        SemanticType::Bytes,
        int(),
        SemanticType::Bool,
        SemanticType::Bool,
    ];
    let mut cond = G::new(
        "validator-type-closure-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let pending = cond.compare(Operation::Gt, input(3), zero);
    let cap = cond.n(131072);
    let bounded = cond.compare(Operation::Lt, input(6), cap);
    let more = cond.and(pending, bounded);
    let more = cond.and(more, input(7));
    let mut body = G::new("validator-type-closure-body", state.clone(), state.clone());
    let tag = body.byte(input(2));
    let sixteen = body.n(16);
    let supported = body.compare(Operation::Lt, tag.clone(), sixteen);
    let twelve = body.n(12);
    let named = body.compare(Operation::Ge, tag.clone(), twelve);
    let thirteen = body.n(13);
    let named_upper = body.compare(Operation::Le, tag.clone(), thirteen);
    let named = body.and(named, named_upper);
    let nine = body.n(9);
    let scalar = body.compare(Operation::Lt, tag, nine);
    let payload_valid = select(
        &mut body,
        "validator-type-scalar-valid",
        "validator-schema-ordinary-type",
        scalar,
        vec![input(0), input(1), input(2)],
    );
    let bind = body.and(named, input(8));
    let named_valid = select(
        &mut body,
        "validator-schema-named-type",
        "validator-schema-ordinary-type",
        bind,
        vec![input(0), input(1), input(2)],
    );
    let id = body.b.graph.nodes.len() as u32 + 1;
    body.op(
        Operation::Subgraph("reader-type-layout-step".into()),
        vec![
            (input(0), SemanticType::Bytes),
            (input(2), int()),
            (input(3), int()),
        ],
        SemanticType::Bytes,
    );
    body.b.graph.nodes.last_mut().unwrap().outputs =
        vec![port(0, SemanticType::Bytes), port(1, int()), port(2, int())];
    let next = SourceEndpoint::NodeOutput { node: id, port: 1 };
    let pending = SourceEndpoint::NodeOutput { node: id, port: 2 };
    let plus = body.advance(pending.clone(), 1);
    let arity = body.arithmetic(Operation::Sub, plus, input(3));
    let zero = body.n(0);
    let children = body.compare(Operation::Gt, arity.clone(), zero);
    let bound = body.n(128);
    let depth_ok = body.compare(Operation::Lt, input(4), bound.clone());
    let valid = body.and(input(7), supported);
    let valid = body.and(valid, named_valid);
    let valid = body.and(valid, payload_valid);
    let valid = body.and(valid, depth_ok);
    let remaining = byte(&mut body, input(5), input(4));
    let one = body.n(1);
    let remaining = body.arithmetic(Operation::Sub, remaining, one);
    let resumed = call(
        &mut body,
        "type-depth-stack-set",
        vec![input(5), input(4), remaining],
        SemanticType::Bytes,
    );
    let ascended = body.loop_node(
        "type-depth-ascend",
        ascend_state,
        vec![resumed.clone(), input(4)],
    );
    let deep = body.compare(Operation::Ge, input(4), bound.clone());
    let child_depth = body.advance(input(4), 1);
    let child_depth = body.pick("reader-pick-offset", deep, child_depth, bound);
    let child_stack = call(
        &mut body,
        "type-depth-stack-set",
        vec![resumed.clone(), child_depth.clone(), arity],
        SemanticType::Bytes,
    );
    let next_stack = choose_bytes(&mut body, children.clone(), resumed, child_stack);
    let depth = body.pick(
        "reader-pick-offset",
        children,
        ascended[1].clone(),
        child_depth,
    );
    let count = body.advance(input(6), 1);
    let mut main = G::new(
        "validator-type-closure-walk",
        vec![SemanticType::Bytes, rows(), int(), SemanticType::Bool],
        vec![SemanticType::Bool],
    );
    let one = main.n(1);
    let zero = main.n(0);
    let mut initial = vec![0; 129];
    initial[0] = 1;
    let stack = main.op(
        Operation::Const(Literal::Bytes(initial)),
        vec![],
        SemanticType::Bytes,
    );
    let yes = main.bool(true);
    let out = main.loop_node(
        "validator-type-closure",
        state,
        vec![
            input(0),
            input(1),
            input(2),
            one,
            zero.clone(),
            stack,
            zero,
            yes,
            input(3),
        ],
    );
    let zero = main.n(0);
    let complete = main.compare(Operation::Eq, out[3].clone(), zero);
    let result = main.and(complete, out[7].clone());
    let mut scalar = G::new(
        "validator-type-scalar-valid",
        vec![SemanticType::Bytes, rows(), int()],
        vec![SemanticType::Bool],
    );
    let end = call(&mut scalar, "reader-type", vec![input(0), input(2)], int());
    let len = scalar.length(input(0));
    let scalar_valid = scalar.compare(Operation::Le, end, len);
    let mut scalar_entry = G::new(
        "validator-type-closure-scalar",
        vec![SemanticType::Bytes, rows(), int(), SemanticType::Bool],
        vec![SemanticType::Bool],
    );
    let scalar_result = call(
        &mut scalar_entry,
        "validator-type-scalar-valid",
        vec![input(0), input(1), input(2)],
        SemanticType::Bool,
    );
    let mut entry = G::new(
        "validator-type-closure",
        vec![SemanticType::Bytes, rows(), int(), SemanticType::Bool],
        vec![SemanticType::Bool],
    );
    let tag = entry.byte(input(2));
    let nine = entry.n(9);
    let scalar_type = entry.compare(Operation::Lt, tag, nine);
    let closure_result = select(
        &mut entry,
        "validator-type-closure-scalar",
        "validator-type-closure-walk",
        scalar_type,
        vec![input(0), input(1), input(2), input(3)],
    );
    let mut profile = G::new(
        "validator-type-profile",
        vec![SemanticType::Bytes, int()],
        vec![SemanticType::Bool],
    );
    let empty = profile.op(Operation::MakeArray, vec![], rows());
    let no = profile.bool(false);
    let profile_result = call(
        &mut profile,
        "validator-type-closure",
        vec![input(0), empty, input(1), no],
        SemanticType::Bool,
    );
    let mut schema = G::new(
        "validator-schema-type",
        vec![SemanticType::Bytes, rows(), int()],
        vec![SemanticType::Bool],
    );
    let yes = schema.bool(true);
    let schema_result = call(
        &mut schema,
        "validator-type-closure",
        vec![input(0), input(1), input(2), yes],
        SemanticType::Bool,
    );
    vec![
        set.finish(vec![stack_set]),
        ac.finish(vec![ascend_more]),
        ab.finish(vec![input(0), parent]),
        left.finish(vec![input(0)]),
        right.finish(vec![input(1)]),
        cond.finish(vec![more]),
        body.finish(vec![
            input(0),
            input(1),
            next,
            pending,
            depth,
            next_stack,
            count,
            valid,
            input(8),
        ]),
        main.finish(vec![result]),
        scalar.finish(vec![scalar_valid]),
        scalar_entry.finish(vec![scalar_result]),
        entry.finish(vec![closure_result]),
        profile.finish(vec![profile_result]),
        schema.finish(vec![schema_result]),
    ]
}
