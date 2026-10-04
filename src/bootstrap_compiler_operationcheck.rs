//! Pure operation proofs performed by executable G0 graphs.
//! Rust only constructs definitions; no source input is interpreted here.
use super::*;
fn descriptor_type() -> SemanticType {
    SemanticType::Slice(Box::new(int()))
}
fn signature() -> Vec<SemanticType> {
    vec![SemanticType::Bytes, descriptor_type()]
}
fn call(g: &mut G, name: &str, args: Vec<SourceEndpoint>, output: SemanticType) -> SourceEndpoint {
    let args = args
        .into_iter()
        .map(|e| {
            let mut t = g.ty(&e);
            if matches!(&t,SemanticType::Integer(r) if r.min>=0&&r.max<=(1_i128<<48)) {
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
        if matches!(&t,SemanticType::Integer(r) if r.min>=0&&r.max<=(1_i128<<48)) {
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
    let n = g.b.graph.nodes.last_mut().unwrap();
    n.inputs[0].name = "selector".into();
    for (i, p) in n.inputs[1..].iter_mut().enumerate() {
        p.name = format!("p{i}");
    }
    value
}
fn constant(g: &mut G, n: i128) -> SourceEndpoint {
    g.op(
        Operation::Const(Literal::Integer(n)),
        vec![],
        full_integer(),
    )
}
fn equal(g: &mut G, e: SourceEndpoint, n: i128) -> SourceEndpoint {
    let n = g.n(n);
    g.compare(Operation::Eq, e, n)
}
fn kind(g: &mut G, at: SourceEndpoint, n: i128) -> SourceEndpoint {
    let tag = g.byte(at);
    equal(g, tag, n)
}
fn type_at(g: &mut G, list_field: u16, ordinal: i128) -> SourceEndpoint {
    let list = g.field(input(1), list_field.into());
    let index = g.n(ordinal);
    call(
        g,
        "validator-port-type-at",
        vec![input(0), list, index],
        int(),
    )
}
fn read_integer(g: &mut G, at: SourceEndpoint) -> SourceEndpoint {
    call(g, "reader-i128", vec![input(0), at], full_integer())
}
fn bounds(g: &mut G, at: SourceEndpoint) -> (SourceEndpoint, SourceEndpoint) {
    let min_at = g.advance(at.clone(), 1);
    let max_at = g.advance(at, 17);
    (read_integer(g, min_at), read_integer(g, max_at))
}
fn all(g: &mut G, flags: Vec<SourceEndpoint>) -> SourceEndpoint {
    flags
        .into_iter()
        .reduce(|a, b| g.and(a, b))
        .unwrap_or_else(|| g.bool(true))
}
fn same_type(g: &mut G, a: SourceEndpoint, b: SourceEndpoint) -> SourceEndpoint {
    let ab = call(
        g,
        "validator-type-assignable",
        vec![input(0), a.clone(), b.clone()],
        SemanticType::Bool,
    );
    let ba = call(
        g,
        "validator-type-assignable",
        vec![input(0), b, a],
        SemanticType::Bool,
    );
    g.and(ab, ba)
}
fn operation_at(g: &mut G) -> SourceEndpoint {
    g.field(input(1), 2)
}
fn fixed_shape(g: &mut G, inputs: Option<usize>) -> SourceEndpoint {
    let i = g.field(input(1), 3);
    let n = g.u32(i);
    let o = g.field(input(1), 4);
    let m = g.u32(o);
    let out = equal(g, m, 1);
    if let Some(inputs) = inputs {
        let ins = equal(g, n, inputs as i128);
        g.and(ins, out)
    } else {
        out
    }
}
fn boolean_graph(tag: u8, count: usize) -> Graph {
    let mut g = G::new(
        &format!("operation-types-{tag}"),
        signature(),
        vec![SemanticType::Bool],
    );
    let mut flags = vec![];
    for i in 0..count {
        let at = type_at(&mut g, 3, i as i128);
        flags.push(kind(&mut g, at, 0));
    }
    let output = type_at(&mut g, 4, 0);
    flags.push(kind(&mut g, output, 0));
    let result = all(&mut g, flags);
    g.finish(vec![result])
}
fn constant_graphs() -> Vec<Graph> {
    let mut integer = G::new(
        "operation-constant-integer",
        signature(),
        vec![SemanticType::Bool],
    );
    let op = operation_at(&mut integer);
    let literal_at = integer.advance(op, 2);
    let value = read_integer(&mut integer, literal_at);
    let out = type_at(&mut integer, 4, 0);
    let (min, max) = bounds(&mut integer, out);
    let above = integer.compare(Operation::Ge, value.clone(), min);
    let below = integer.compare(Operation::Le, value, max);
    let integer_ok = integer.and(above, below);
    let mut integer_guard = G::new(
        "operation-constant-integer-guard",
        signature(),
        vec![SemanticType::Bool],
    );
    let out = type_at(&mut integer_guard, 4, 0);
    let is_integer = kind(&mut integer_guard, out, 1);
    let integer_guarded = choose(
        &mut integer_guard,
        is_integer,
        "operation-constant-integer",
        "operation-invalid",
        vec![input(0), input(1)],
        SemanticType::Bool,
    );
    let mut graphs = vec![
        integer.finish(vec![integer_ok]),
        integer_guard.finish(vec![integer_guarded]),
    ];
    for (literal, output) in [(0, 0), (2, 7), (3, 8)] {
        let mut g = G::new(
            &format!("operation-constant-{literal}"),
            signature(),
            vec![SemanticType::Bool],
        );
        let out = type_at(&mut g, 4, 0);
        let mut result = kind(&mut g, out, output);
        if literal == 0 {
            let op = operation_at(&mut g);
            let at = g.advance(op, 2);
            let b = g.byte(at);
            let one = g.n(1);
            let valid = g.compare(Operation::Le, b, one);
            result = g.and(result, valid);
        }
        graphs.push(g.finish(vec![result]));
    }
    let mut main = G::new("operation-types-0", signature(), vec![SemanticType::Bool]);
    let op = operation_at(&mut main);
    let at = main.advance(op, 1);
    let tag = main.byte(at);
    let test = equal(&mut main, tag, 0);
    let result = choose(
        &mut main,
        test,
        "operation-constant-0",
        "operation-constant-rest",
        vec![input(0), input(1)],
        SemanticType::Bool,
    );
    graphs.push(main.finish(vec![result]));
    for (name, literal, yes, no) in [
        (
            "operation-constant-rest",
            1,
            "operation-constant-integer-guard",
            "operation-constant-tail",
        ),
        (
            "operation-constant-tail",
            2,
            "operation-constant-2",
            "operation-constant-last",
        ),
        (
            "operation-constant-last",
            3,
            "operation-constant-3",
            "operation-invalid",
        ),
    ] {
        let mut g = G::new(name, signature(), vec![SemanticType::Bool]);
        let op = operation_at(&mut g);
        let at = g.advance(op, 1);
        let tag = g.byte(at);
        let test = equal(&mut g, tag, literal);
        let result = choose(
            &mut g,
            test,
            yes,
            no,
            vec![input(0), input(1)],
            SemanticType::Bool,
        );
        graphs.push(g.finish(vec![result]));
    }
    graphs
}
fn checked(
    g: &mut G,
    op: Operation,
    a: SourceEndpoint,
    b: SourceEndpoint,
) -> (SourceEndpoint, SourceEndpoint) {
    let ty = SemanticType::Result(Box::new(full_integer()), Box::new(SemanticType::Bool));
    let result = g.op(
        op,
        vec![(a, full_integer()), (b, full_integer())],
        ty.clone(),
    );
    let ok = g.op(
        Operation::ResultIsOk,
        vec![(result.clone(), ty.clone())],
        SemanticType::Bool,
    );
    let zero = constant(g, 0);
    let value = g.op(
        Operation::UnwrapOr,
        vec![(result, ty), (zero, full_integer())],
        full_integer(),
    );
    (ok, value)
}
fn arithmetic_graphs() -> Vec<Graph> {
    let mut graphs = vec![];
    for (tag, op) in [
        (1, Operation::CheckedAdd),
        (2, Operation::CheckedSub),
        (3, Operation::CheckedMul),
    ] {
        let mut g = G::new(
            &format!("operation-arithmetic-{tag}"),
            signature(),
            vec![SemanticType::Bool],
        );
        let a = type_at(&mut g, 3, 0);
        let b = type_at(&mut g, 3, 1);
        let output = type_at(&mut g, 4, 0);
        let (amin, amax) = bounds(&mut g, a);
        let (bmin, bmax) = bounds(&mut g, b);
        let (omin, omax) = bounds(&mut g, output);
        let corners = match tag {
            1 => vec![(amin, bmin), (amax, bmax)],
            2 => vec![(amin, bmax), (amax, bmin)],
            _ => vec![
                (amin.clone(), bmin.clone()),
                (amin, bmax.clone()),
                (amax.clone(), bmin),
                (amax, bmax),
            ],
        };
        let mut flags = vec![];
        for (a, b) in corners {
            let (ok, value) = checked(&mut g, op.clone(), a, b);
            let lower = g.compare(Operation::Ge, value.clone(), omin.clone());
            let upper = g.compare(Operation::Le, value, omax.clone());
            flags.extend([ok, lower, upper]);
        }
        let result = all(&mut g, flags);
        graphs.push(g.finish(vec![result]));
        let mut guard = G::new(
            &format!("operation-types-{tag}"),
            signature(),
            vec![SemanticType::Bool],
        );
        let a = type_at(&mut guard, 3, 0);
        let b = type_at(&mut guard, 3, 1);
        let out = type_at(&mut guard, 4, 0);
        let flags = [a, b, out]
            .into_iter()
            .map(|at| kind(&mut guard, at, 1))
            .collect();
        let valid = all(&mut guard, flags);
        let result = choose(
            &mut guard,
            valid,
            &format!("operation-arithmetic-{tag}"),
            "operation-invalid",
            vec![input(0), input(1)],
            SemanticType::Bool,
        );
        graphs.push(guard.finish(vec![result]));
    }
    for tag in 17..=21 {
        let mut g = G::new(
            &format!("operation-types-{tag}"),
            signature(),
            vec![SemanticType::Bool],
        );
        let a = type_at(&mut g, 3, 0);
        let b = type_at(&mut g, 3, 1);
        let out = type_at(&mut g, 4, 0);
        let ai = kind(&mut g, a.clone(), 1);
        let bi = kind(&mut g, b.clone(), 1);
        let same = same_type(&mut g, a, b);
        let ob = kind(&mut g, out, 0);
        let result = all(&mut g, vec![ai, bi, same, ob]);
        graphs.push(g.finish(vec![result]));
    }
    let mut conversion = G::new("operation-types-28", signature(), vec![SemanticType::Bool]);
    let a = type_at(&mut conversion, 3, 0);
    let out = type_at(&mut conversion, 4, 0);
    let a = kind(&mut conversion, a, 1);
    let b = kind(&mut conversion, out, 1);
    let valid = conversion.and(a, b);
    graphs.push(conversion.finish(vec![valid]));
    let mut proof = G::new(
        "operation-checked-result",
        signature(),
        vec![SemanticType::Bool],
    );
    let out = type_at(&mut proof, 4, 0);
    let inner = proof.advance(out, 1);
    let (min, max) = bounds(&mut proof, inner);
    let want_min = constant(&mut proof, i128::MIN);
    let want_max = constant(&mut proof, i128::MAX);
    let min_ok = proof.compare(Operation::Eq, min, want_min);
    let max_ok = proof.compare(Operation::Eq, max, want_max);
    let valid = proof.and(min_ok, max_ok);
    graphs.push(proof.finish(vec![valid]));
    for tag in 70..=72 {
        let mut g = G::new(
            &format!("operation-types-{tag}"),
            signature(),
            vec![SemanticType::Bool],
        );
        let a = type_at(&mut g, 3, 0);
        let b = type_at(&mut g, 3, 1);
        let out = type_at(&mut g, 4, 0);
        let inner = g.advance(out.clone(), 1);
        let err = g.advance(out.clone(), 34);
        let ai = kind(&mut g, a, 1);
        let bi = kind(&mut g, b, 1);
        let result_kind = kind(&mut g, out, 15);
        let inner_integer = kind(&mut g, inner, 1);
        let err_bool = kind(&mut g, err, 0);
        let flags = all(&mut g, vec![ai, bi, result_kind, inner_integer, err_bool]);
        let valid = choose(
            &mut g,
            flags,
            "operation-checked-result",
            "operation-invalid",
            vec![input(0), input(1)],
            SemanticType::Bool,
        );
        graphs.push(g.finish(vec![valid]));
    }
    let mut is_ok = G::new("operation-types-73", signature(), vec![SemanticType::Bool]);
    let a = type_at(&mut is_ok, 3, 0);
    let out = type_at(&mut is_ok, 4, 0);
    let input_result = kind(&mut is_ok, a, 15);
    let output_bool = kind(&mut is_ok, out, 0);
    let valid = is_ok.and(input_result, output_bool);
    graphs.push(is_ok.finish(vec![valid]));
    graphs
}
fn assignable(g: &mut G, a: SourceEndpoint, b: SourceEndpoint) -> SourceEndpoint {
    call(
        g,
        "validator-type-assignable",
        vec![input(0), a, b],
        SemanticType::Bool,
    )
}
fn sequence(g: &mut G, at: SourceEndpoint) -> (SourceEndpoint, SourceEndpoint) {
    let array = kind(g, at.clone(), 9);
    let slice = kind(g, at.clone(), 10);
    let vector = kind(g, at.clone(), 11);
    let long = g.logic(Operation::Or, array, vector);
    let yes = g.logic(Operation::Or, long.clone(), slice);
    let short_at = g.advance(at.clone(), 1);
    let long_at = g.advance(at, 9);
    let inner = g.pick("reader-pick-offset", long, short_at, long_at);
    (yes, inner)
}
fn u64_graph() -> Graph {
    let mut g = G::new(
        "operation-u64",
        vec![SemanticType::Bytes, int()],
        vec![full_integer()],
    );
    let low = g.u32(input(1));
    let next = g.advance(input(1), 4);
    let high = g.u32(next);
    let narrow = SemanticType::Integer(IntegerType {
        min: 0,
        max: u32::MAX as i128,
    });
    let low = g.op(
        Operation::ConvertChecked,
        vec![(low, int())],
        narrow.clone(),
    );
    let high = g.op(
        Operation::ConvertChecked,
        vec![(high, int())],
        narrow.clone(),
    );
    let base_type = SemanticType::Integer(IntegerType {
        min: 4294967296,
        max: 4294967296,
    });
    let base = g.op(
        Operation::Const(Literal::Integer(4294967296)),
        vec![],
        base_type.clone(),
    );
    let scaled_type = SemanticType::Integer(IntegerType {
        min: 0,
        max: (u32::MAX as i128) * 4294967296,
    });
    let high = g.op(
        Operation::Mul,
        vec![(high, narrow.clone()), (base, base_type)],
        scaled_type.clone(),
    );
    let sum = g.op(
        Operation::Add,
        vec![(low, narrow), (high, scaled_type)],
        full_integer(),
    );
    g.finish(vec![sum])
}
fn array_member_graphs() -> Vec<Graph> {
    let state = vec![
        SemanticType::Bytes,
        int(),
        int(),
        int(),
        int(),
        SemanticType::Bool,
    ];
    let mut cond = G::new(
        "operation-array-members-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let test = cond.compare(Operation::Lt, input(4), input(3));
    let mut body = G::new("operation-array-members-body", state.clone(), state.clone());
    let at = call(
        &mut body,
        "validator-port-type-at",
        vec![input(0), input(1), input(4)],
        int(),
    );
    let compatible = assignable(&mut body, at, input(2));
    let valid = body.and(input(5), compatible);
    let next = body.advance(input(4), 1);
    let mut main = G::new(
        "operation-array-members",
        vec![SemanticType::Bytes, int(), int()],
        vec![SemanticType::Bool],
    );
    let count = main.u32(input(1));
    let zero = main.n(0);
    let yes = main.bool(true);
    let out = main.loop_node(
        "operation-array-members",
        state,
        vec![input(0), input(1), input(2), count, zero, yes],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), input(2), input(3), next, valid]),
        main.finish(vec![out[5].clone()]),
    ]
}
fn composite_graphs() -> Vec<Graph> {
    let mut graphs = vec![u64_graph()];
    graphs.extend(array_member_graphs());
    for tag in [
        30, 31, 32, 33, 34, 35, 36, 37, 42, 43, 44, 45, 46, 48, 66, 67, 68, 69,
    ] {
        let mut g = G::new(
            &format!("operation-types-{tag}"),
            signature(),
            vec![SemanticType::Bool],
        );
        let output = type_at(&mut g, 4, 0);
        let count = if tag == 30 || tag == 43 {
            0
        } else if matches!(tag, 31 | 33 | 34 | 46 | 48 | 67) {
            2
        } else if tag == 66 {
            3
        } else {
            1
        };
        let inputs: Vec<_> = (0..count).map(|i| type_at(&mut g, 3, i)).collect();
        let result = match tag {
            33 | 34 => {
                let expected = if tag == 33 { 7 } else { 8 };
                let mut flags: Vec<_> = inputs
                    .into_iter()
                    .map(|at| kind(&mut g, at, expected))
                    .collect();
                flags.push(kind(&mut g, output, expected));
                all(&mut g, flags)
            }
            35 => {
                let a = kind(&mut g, inputs[0].clone(), 7);
                let o = kind(&mut g, output, 8);
                g.and(a, o)
            }
            36 => {
                let a = kind(&mut g, inputs[0].clone(), 8);
                let o = kind(&mut g, output.clone(), 15);
                let ok = g.advance(output.clone(), 1);
                let err = g.advance(output, 2);
                let ok = kind(&mut g, ok, 7);
                let err = kind(&mut g, err, 8);
                all(&mut g, vec![a, o, ok, err])
            }
            37 => {
                let a = kind(&mut g, inputs[0].clone(), 1);
                let o = kind(&mut g, output, 7);
                g.and(a, o)
            }
            43 => kind(&mut g, output, 14),
            66 => {
                let a = kind(&mut g, inputs[0].clone(), 8);
                let b = kind(&mut g, inputs[1].clone(), 1);
                let c = kind(&mut g, inputs[2].clone(), 1);
                let o = kind(&mut g, output, 8);
                all(&mut g, vec![a, b, c, o])
            }
            48 => {
                let (seq, inner) = sequence(&mut g, inputs[0].clone());
                let text = kind(&mut g, inner, 7);
                let separator = kind(&mut g, inputs[1].clone(), 7);
                let o = kind(&mut g, output, 7);
                all(&mut g, vec![seq, text, separator, o])
            }
            _ => {
                let guard = match tag {
                    30 => sequence(&mut g, output).0,
                    31 => {
                        let (seq, _) = sequence(&mut g, inputs[0].clone());
                        let bytes = kind(&mut g, inputs[0].clone(), 8);
                        let index = kind(&mut g, inputs[1].clone(), 1);
                        let option = kind(&mut g, output, 14);
                        let collection = g.logic(Operation::Or, seq, bytes);
                        all(&mut g, vec![collection, index, option])
                    }
                    32 => {
                        let (seq, _) = sequence(&mut g, inputs[0].clone());
                        let bytes = kind(&mut g, inputs[0].clone(), 8);
                        let collection = g.logic(Operation::Or, seq, bytes);
                        let integer = kind(&mut g, output, 1);
                        g.and(collection, integer)
                    }
                    42 => kind(&mut g, output, 14),
                    44 | 45 => kind(&mut g, output, 15),
                    46 => {
                        let option = kind(&mut g, inputs[0].clone(), 14);
                        let result = kind(&mut g, inputs[0].clone(), 15);
                        g.logic(Operation::Or, option, result)
                    }
                    67 => {
                        let (a, _) = sequence(&mut g, inputs[0].clone());
                        let (b, _) = sequence(&mut g, inputs[1].clone());
                        let o = kind(&mut g, output, 10);
                        all(&mut g, vec![a, b, o])
                    }
                    68 => {
                        let a = kind(&mut g, inputs[0].clone(), 1);
                        let o = kind(&mut g, output.clone(), 10);
                        let inner = g.advance(output, 1);
                        let integer = kind(&mut g, inner, 1);
                        all(&mut g, vec![a, o, integer])
                    }
                    69 => {
                        let (a, inner) = sequence(&mut g, inputs[0].clone());
                        let integer = kind(&mut g, inner, 1);
                        let o = kind(&mut g, output, 8);
                        all(&mut g, vec![a, integer, o])
                    }
                    _ => unreachable!(),
                };
                choose(
                    &mut g,
                    guard,
                    &format!("operation-proof-{tag}"),
                    "operation-invalid",
                    vec![input(0), input(1)],
                    SemanticType::Bool,
                )
            }
        };
        graphs.push(g.finish(vec![result]));
    }
    for tag in [30, 31, 32, 42, 44, 45, 46, 67, 68, 69] {
        let mut g = G::new(
            &format!("operation-proof-{tag}"),
            signature(),
            vec![SemanticType::Bool],
        );
        let output = type_at(&mut g, 4, 0);
        let result = match tag {
            30 => {
                let (_, inner) = sequence(&mut g, output.clone());
                let input_list = g.field(input(1), 3);
                let members = call(
                    &mut g,
                    "operation-array-members",
                    vec![input(0), input_list.clone(), inner],
                    SemanticType::Bool,
                );
                let slice = kind(&mut g, output.clone(), 10);
                let length_at = g.advance(output, 1);
                let length = call(
                    &mut g,
                    "operation-u64",
                    vec![input(0), length_at],
                    full_integer(),
                );
                let n = g.u32(input_list);
                let right_length = g.compare(Operation::Eq, length, n);
                let length_ok = g.logic(Operation::Or, slice, right_length);
                g.and(members, length_ok)
            }
            31 => {
                let input = type_at(&mut g, 3, 0);
                let (bytes, inner) = sequence(&mut g, input.clone());
                let _ = bytes;
                let is_bytes = kind(&mut g, input, 8);
                let out_inner = g.advance(output, 1);
                let array_ok = assignable(&mut g, inner, out_inner.clone());
                let out_integer = kind(&mut g, out_inner.clone(), 1);
                let (min, max) = bounds(&mut g, out_inner);
                let zero = constant(&mut g, 0);
                let limit = constant(&mut g, 255);
                let lower = g.compare(Operation::Le, min, zero);
                let upper = g.compare(Operation::Ge, max, limit);
                let byte_ok = all(&mut g, vec![out_integer, lower, upper]);
                g.pick("operation-pick-bool", is_bytes, array_ok, byte_ok)
            }
            32 => {
                let (min, max) = bounds(&mut g, output);
                let zero = constant(&mut g, 0);
                let maxlen = constant(&mut g, u64::MAX as i128);
                let lower = g.compare(Operation::Eq, min, zero);
                let upper = g.compare(Operation::Ge, max, maxlen);
                g.and(lower, upper)
            }
            42 | 44 | 45 => {
                let source_type = type_at(&mut g, 3, 0);
                let inner = g.advance(output, 1);
                let inner = if tag == 45 {
                    call(&mut g, "reader-type-layout", vec![input(0), inner], int())
                } else {
                    inner
                };
                assignable(&mut g, source_type, inner)
            }
            46 => {
                let first = type_at(&mut g, 3, 0);
                let fallback = type_at(&mut g, 3, 1);
                let inner = g.advance(first, 1);
                let value_ok = assignable(&mut g, inner, output.clone());
                let fallback_ok = assignable(&mut g, fallback, output);
                g.and(value_ok, fallback_ok)
            }
            67 => {
                let a = type_at(&mut g, 3, 0);
                let b = type_at(&mut g, 3, 1);
                let (_, a) = sequence(&mut g, a);
                let (_, b) = sequence(&mut g, b);
                let inner = g.advance(output, 1);
                let a = assignable(&mut g, a, inner.clone());
                let b = assignable(&mut g, b, inner);
                g.and(a, b)
            }
            68 => {
                let count = type_at(&mut g, 3, 0);
                let (min, max) = bounds(&mut g, count);
                let inner = g.advance(output, 1);
                let (omin, omax) = bounds(&mut g, inner);
                let zero = constant(&mut g, 0);
                let one = constant(&mut g, 1);
                let nonnegative = g.compare(Operation::Ge, min, zero.clone());
                let lower = g.compare(Operation::Eq, omin, zero);
                let (_, last) = checked(&mut g, Operation::CheckedSub, max, one);
                let upper = g.compare(Operation::Ge, omax, last);
                all(&mut g, vec![nonnegative, lower, upper])
            }
            69 => {
                let input = type_at(&mut g, 3, 0);
                let (_, inner) = sequence(&mut g, input);
                let (min, max) = bounds(&mut g, inner);
                let zero = constant(&mut g, 0);
                let byte = constant(&mut g, 255);
                let lower = g.compare(Operation::Ge, min, zero);
                let upper = g.compare(Operation::Le, max, byte);
                g.and(lower, upper)
            }
            _ => unreachable!(),
        };
        graphs.push(g.finish(vec![result]));
    }
    for (name, ordinal) in [
        ("operation-pick-bool-first", 0),
        ("operation-pick-bool-second", 1),
    ] {
        let g = G::new(
            name,
            vec![SemanticType::Bool, SemanticType::Bool],
            vec![SemanticType::Bool],
        );
        graphs.push(g.finish(vec![input(ordinal)]));
    }
    graphs
}
fn choose_pair(
    g: &mut G,
    test: SourceEndpoint,
    yes: &str,
    no: &str,
    args: Vec<SourceEndpoint>,
) -> (SourceEndpoint, SourceEndpoint) {
    let id = g.b.graph.nodes.len() as u32 + 1;
    let first = choose(g, test, yes, no, args, SemanticType::Bool);
    g.b.graph.nodes.last_mut().unwrap().outputs =
        vec![port(0, SemanticType::Bool), port(1, full_integer())];
    (first, SourceEndpoint::NodeOutput { node: id, port: 1 })
}
fn integer_pick(
    g: &mut G,
    test: SourceEndpoint,
    no: SourceEndpoint,
    yes: SourceEndpoint,
) -> SourceEndpoint {
    choose(
        g,
        test,
        "operation-pick-integer-second",
        "operation-pick-integer-first",
        vec![no, yes],
        full_integer(),
    )
}
fn extended_arithmetic_graphs() -> Vec<Graph> {
    let mut graphs = vec![];
    for (name, i) in [
        ("operation-pick-integer-first", 0),
        ("operation-pick-integer-second", 1),
    ] {
        let g = G::new(
            name,
            vec![full_integer(), full_integer()],
            vec![full_integer()],
        );
        graphs.push(g.finish(vec![input(i)]));
    }
    let mut bad = G::new(
        "operation-div-point-invalid",
        vec![full_integer(), full_integer()],
        vec![SemanticType::Bool, full_integer()],
    );
    let no = bad.bool(false);
    let zero = constant(&mut bad, 0);
    graphs.push(bad.finish(vec![no, zero]));
    let mut run = G::new(
        "operation-div-point-run",
        vec![full_integer(), full_integer()],
        vec![SemanticType::Bool, full_integer()],
    );
    let yes = run.bool(true);
    let quotient = run.op(
        Operation::Div,
        vec![(input(0), full_integer()), (input(1), full_integer())],
        full_integer(),
    );
    graphs.push(run.finish(vec![yes, quotient]));
    let mut point = G::new(
        "operation-div-point",
        vec![full_integer(), full_integer()],
        vec![SemanticType::Bool, full_integer()],
    );
    let zero = constant(&mut point, 0);
    let nonzero = point.compare(Operation::Eq, input(1), zero);
    let nonzero = point.not(nonzero);
    let min = constant(&mut point, i128::MIN);
    let negative_one = constant(&mut point, -1);
    let amin = point.compare(Operation::Eq, input(0), min);
    let bminus = point.compare(Operation::Eq, input(1), negative_one);
    let overflow = point.and(amin, bminus);
    let safe = point.not(overflow);
    let valid = point.and(nonzero, safe);
    let (ok, value) = choose_pair(
        &mut point,
        valid,
        "operation-div-point-run",
        "operation-div-point-invalid",
        vec![input(0), input(1)],
    );
    graphs.push(point.finish(vec![ok, value]));
    let mut proof = G::new(
        "operation-arithmetic-26",
        signature(),
        vec![SemanticType::Bool],
    );
    let a = type_at(&mut proof, 3, 0);
    let b = type_at(&mut proof, 3, 1);
    let o = type_at(&mut proof, 4, 0);
    let (amin, amax) = bounds(&mut proof, a);
    let (bmin, bmax) = bounds(&mut proof, b);
    let (omin, omax) = bounds(&mut proof, o);
    let neg = constant(&mut proof, -1);
    let one = constant(&mut proof, 1);
    let mut any = proof.bool(false);
    let mut all_ok = proof.bool(true);
    for numerator in [amin, amax] {
        for divisor in [bmin.clone(), bmax.clone(), neg.clone(), one.clone()] {
            let above = proof.compare(Operation::Ge, divisor.clone(), bmin.clone());
            let below = proof.compare(Operation::Le, divisor.clone(), bmax.clone());
            let in_range = proof.and(above, below);
            let id = proof.b.graph.nodes.len() as u32 + 1;
            let point = call(
                &mut proof,
                "operation-div-point",
                vec![numerator.clone(), divisor],
                SemanticType::Bool,
            );
            proof.b.graph.nodes.last_mut().unwrap().outputs =
                vec![port(0, SemanticType::Bool), port(1, full_integer())];
            let quotient = SourceEndpoint::NodeOutput { node: id, port: 1 };
            let used = proof.and(in_range, point);
            any = proof.logic(Operation::Or, any, used.clone());
            let lower = proof.compare(Operation::Ge, quotient.clone(), omin.clone());
            let upper = proof.compare(Operation::Le, quotient, omax.clone());
            let contains = proof.and(lower, upper);
            let ignored = proof.not(used);
            let valid = proof.logic(Operation::Or, ignored, contains);
            all_ok = proof.and(all_ok, valid);
        }
    }
    let proof_ok = proof.and(any, all_ok);
    graphs.push(proof.finish(vec![proof_ok]));
    let mut rem = G::new(
        "operation-arithmetic-27",
        signature(),
        vec![SemanticType::Bool],
    );
    let a = type_at(&mut rem, 3, 0);
    let b = type_at(&mut rem, 3, 1);
    let o = type_at(&mut rem, 4, 0);
    let (amin, amax) = bounds(&mut rem, a);
    let (bmin, bmax) = bounds(&mut rem, b);
    let (omin, omax) = bounds(&mut rem, o);
    let zero = constant(&mut rem, 0);
    let minus_one = constant(&mut rem, -1);
    let one = constant(&mut rem, 1);
    let min = constant(&mut rem, i128::MIN);
    let bzero_min = rem.compare(Operation::Eq, bmin.clone(), zero.clone());
    let bzero_max = rem.compare(Operation::Eq, bmax.clone(), zero.clone());
    let zeros = rem.and(bzero_min, bzero_max);
    let nonzero = rem.not(zeros);
    let amin_min = rem.compare(Operation::Eq, amin.clone(), min.clone());
    let amax_min = rem.compare(Operation::Eq, amax.clone(), min);
    let bmin_minus = rem.compare(Operation::Eq, bmin.clone(), minus_one.clone());
    let bmax_minus = rem.compare(Operation::Eq, bmax.clone(), minus_one.clone());
    let invalid = all(&mut rem, vec![amin_min, amax_min, bmin_minus, bmax_minus]);
    let valid = rem.not(invalid);
    let mut limits = vec![];
    for divisor in [bmin, bmax] {
        let negative = rem.compare(Operation::Lt, divisor.clone(), zero.clone());
        let (_, positive) = checked(
            &mut rem,
            Operation::CheckedSub,
            divisor.clone(),
            one.clone(),
        );
        let (_, negative_value) =
            checked(&mut rem, Operation::CheckedSub, minus_one.clone(), divisor);
        let bound = integer_pick(&mut rem, negative, positive, negative_value);
        let less_zero = rem.compare(Operation::Lt, bound.clone(), zero.clone());
        let bound = integer_pick(&mut rem, less_zero, bound, zero.clone());
        limits.push(bound);
    }
    let bigger = rem.compare(Operation::Gt, limits[1].clone(), limits[0].clone());
    let bound = integer_pick(&mut rem, bigger, limits[0].clone(), limits[1].clone());
    let negative_a = rem.compare(Operation::Lt, amin, zero.clone());
    let positive_a = rem.compare(Operation::Gt, amax, zero.clone());
    let (_, negative_bound) = checked(&mut rem, Operation::CheckedSub, zero.clone(), bound.clone());
    let needed_min = integer_pick(&mut rem, negative_a, zero.clone(), negative_bound);
    let needed_max = integer_pick(&mut rem, positive_a, zero, bound);
    let lower = rem.compare(Operation::Ge, needed_min, omin);
    let upper = rem.compare(Operation::Le, needed_max, omax);
    let rem_ok = all(&mut rem, vec![nonzero, valid, lower, upper]);
    graphs.push(rem.finish(vec![rem_ok]));
    for tag in [26, 27] {
        let mut g = G::new(
            &format!("operation-types-{tag}"),
            signature(),
            vec![SemanticType::Bool],
        );
        let a = type_at(&mut g, 3, 0);
        let b = type_at(&mut g, 3, 1);
        let o = type_at(&mut g, 4, 0);
        let flags = [a, b, o]
            .into_iter()
            .map(|at| kind(&mut g, at, 1))
            .collect();
        let guard = all(&mut g, flags);
        let valid = choose(
            &mut g,
            guard,
            &format!("operation-arithmetic-{tag}"),
            "operation-invalid",
            vec![input(0), input(1)],
            SemanticType::Bool,
        );
        graphs.push(g.finish(vec![valid]));
    }
    graphs
}
fn truncate_graphs() -> Vec<Graph> {
    let state = vec![int(), int(), full_integer()];
    let mut cond = G::new(
        "operation-pow2-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let test = cond.compare(Operation::Lt, input(1), input(0));
    let mut body = G::new("operation-pow2-body", state.clone(), state.clone());
    let two = constant(&mut body, 2);
    let (_, value) = checked(&mut body, Operation::CheckedMul, input(2), two);
    let next = body.advance(input(1), 1);
    let mut power = G::new("operation-pow2", vec![int()], vec![full_integer()]);
    let zero = power.n(0);
    let one = constant(&mut power, 1);
    let out = power.loop_node("operation-pow2", state, vec![input(0), zero, one]);
    let mut proof = G::new(
        "operation-truncate-proof",
        signature(),
        vec![SemanticType::Bool],
    );
    let op = operation_at(&mut proof);
    let bits_at = proof.advance(op.clone(), 1);
    let word = proof.u32(bits_at);
    let modulus = proof.n(65536);
    let bits = proof.op(Operation::Rem, vec![(word, int()), (modulus, int())], int());
    let sign_at = proof.advance(op, 3);
    let sign = proof.byte(sign_at);
    let signed = equal(&mut proof, sign, 1);
    let one = proof.n(1);
    let less = proof.arithmetic(Operation::Sub, bits.clone(), one);
    let exponent = proof.pick("reader-pick-offset", signed.clone(), bits, less);
    let magnitude = call(&mut proof, "operation-pow2", vec![exponent], full_integer());
    let zero = constant(&mut proof, 0);
    let (_, negative) = checked(
        &mut proof,
        Operation::CheckedSub,
        zero.clone(),
        magnitude.clone(),
    );
    let min = integer_pick(&mut proof, signed, zero, negative);
    let one = constant(&mut proof, 1);
    let (_, max) = checked(&mut proof, Operation::CheckedSub, magnitude, one);
    let output = type_at(&mut proof, 4, 0);
    let (omin, omax) = bounds(&mut proof, output);
    let same_min = proof.compare(Operation::Eq, min, omin);
    let same_max = proof.compare(Operation::Eq, max, omax);
    let proof_ok = proof.and(same_min, same_max);
    let mut guard = G::new("operation-types-29", signature(), vec![SemanticType::Bool]);
    let a = type_at(&mut guard, 3, 0);
    let o = type_at(&mut guard, 4, 0);
    let a = kind(&mut guard, a, 1);
    let o = kind(&mut guard, o, 1);
    let op = operation_at(&mut guard);
    let bits_at = guard.advance(op.clone(), 1);
    let word = guard.u32(bits_at);
    let modulus = guard.n(65536);
    let bits = guard.op(Operation::Rem, vec![(word, int()), (modulus, int())], int());
    let one = guard.n(1);
    let sixty_four = guard.n(64);
    let lower = guard.compare(Operation::Ge, bits.clone(), one.clone());
    let upper = guard.compare(Operation::Le, bits, sixty_four);
    let sign_at = guard.advance(op, 3);
    let sign = guard.byte(sign_at);
    let sign_ok = guard.compare(Operation::Le, sign, one);
    let valid = all(&mut guard, vec![a, o, lower, upper, sign_ok]);
    let valid = choose(
        &mut guard,
        valid,
        "operation-truncate-proof",
        "operation-invalid",
        vec![input(0), input(1)],
        SemanticType::Bool,
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), next, value]),
        power.finish(vec![out[2].clone()]),
        proof.finish(vec![proof_ok]),
        guard.finish(vec![valid]),
    ]
}
fn string_graphs() -> Vec<Graph> {
    let state = vec![
        SemanticType::Bytes,
        int(),
        int(),
        int(),
        int(),
        SemanticType::Bool,
    ];
    let mut cond = G::new(
        "operation-string-equal-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let remaining = cond.compare(Operation::Lt, input(3), input(4));
    let test = cond.and(remaining, input(5));
    let mut body = G::new("operation-string-equal-body", state.clone(), state.clone());
    let a = body.arithmetic(Operation::Add, input(1), input(3));
    let b = body.arithmetic(Operation::Add, input(2), input(3));
    let a = body.byte(a);
    let b = body.byte(b);
    let same = body.compare(Operation::Eq, a, b);
    let valid = body.and(input(5), same);
    let next = body.advance(input(3), 1);
    let mut main = G::new(
        "operation-string-equal",
        vec![SemanticType::Bytes, int(), int()],
        vec![SemanticType::Bool],
    );
    let a_len = main.u32(input(1));
    let b_len = main.u32(input(2));
    let same_len = main.compare(Operation::Eq, a_len.clone(), b_len);
    let a = main.advance(input(1), 4);
    let b = main.advance(input(2), 4);
    let zero = main.n(0);
    let out = main.loop_node(
        "operation-string-equal",
        state,
        vec![input(0), a, b, zero, a_len, same_len],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), input(2), next, input(4), valid]),
        main.finish(vec![out[5].clone()]),
    ]
}
fn distinct_field_graphs() -> Vec<Graph> {
    let state = vec![SemanticType::Bytes, int(), int(), int(), SemanticType::Bool];
    let mut cond = G::new(
        "operation-field-unseen-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let left = cond.compare(Operation::Gt, input(3), zero);
    let test = cond.and(left, input(4));
    let mut body = G::new("operation-field-unseen-body", state.clone(), state.clone());
    let same = call(
        &mut body,
        "operation-string-equal",
        vec![input(0), input(1), input(2)],
        SemanticType::Bool,
    );
    let different = body.not(same);
    let valid = body.and(input(4), different);
    let next = body.skip_blob(input(2));
    let one = body.n(1);
    let inner_left = body.arithmetic(Operation::Sub, input(3), one);
    let mut main = G::new(
        "operation-field-unseen",
        vec![SemanticType::Bytes, int(), int(), int()],
        vec![SemanticType::Bool],
    );
    let yes = main.bool(true);
    let out = main.loop_node(
        "operation-field-unseen",
        state,
        vec![input(0), input(1), input(2), input(3), yes],
    );
    let state = vec![
        SemanticType::Bytes,
        int(),
        int(),
        int(),
        int(),
        SemanticType::Bool,
    ];
    let mut outer_cond = G::new(
        "operation-distinct-fields-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = outer_cond.n(0);
    let left = outer_cond.compare(Operation::Gt, input(3), zero);
    let test_outer = outer_cond.and(left, input(5));
    let mut outer_body = G::new(
        "operation-distinct-fields-body",
        state.clone(),
        state.clone(),
    );
    let unique = call(
        &mut outer_body,
        "operation-field-unseen",
        vec![input(0), input(2), input(1), input(4)],
        SemanticType::Bool,
    );
    let valid_outer = outer_body.and(input(5), unique);
    let next_outer = outer_body.skip_blob(input(2));
    let one = outer_body.n(1);
    let left_outer = outer_body.arithmetic(Operation::Sub, input(3), one);
    let index = outer_body.advance(input(4), 1);
    let mut outer = G::new(
        "operation-distinct-fields",
        vec![SemanticType::Bytes, int()],
        vec![SemanticType::Bool],
    );
    let count = outer.u32(input(1));
    let first = outer.advance(input(1), 4);
    let zero = outer.n(0);
    let yes = outer.bool(true);
    let result = outer.loop_node(
        "operation-distinct-fields",
        state,
        vec![input(0), first.clone(), first, count, zero, yes],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), next, inner_left, valid]),
        main.finish(vec![out[4].clone()]),
        outer_cond.finish(vec![test_outer]),
        outer_body.finish(vec![
            input(0),
            input(1),
            next_outer,
            left_outer,
            index,
            valid_outer,
        ]),
        outer.finish(vec![result[5].clone()]),
    ]
}
fn named_graphs() -> Vec<Graph> {
    // The graph-local interface proves local contracts. Program-level schema
    // checks must also resolve named fields, required fields and payload types.
    let mut graphs = string_graphs();
    graphs.extend(distinct_field_graphs());
    for tag in [38, 40] {
        let mut proof = G::new(
            &format!("operation-named-proof-{tag}"),
            signature(),
            vec![SemanticType::Bool],
        );
        let op = operation_at(&mut proof);
        let schema = proof.advance(op, 1);
        let len = proof.u32(schema.clone());
        let zero = proof.n(0);
        let nonempty = proof.compare(Operation::Gt, len, zero);
        let output = type_at(&mut proof, 4, 0);
        let out_schema = proof.advance(output, 1);
        let same = call(
            &mut proof,
            "operation-string-equal",
            vec![input(0), schema.clone(), out_schema],
            SemanticType::Bool,
        );
        let next = proof.skip_blob(schema);
        let extra = if tag == 38 {
            let list = proof.field(input(1), 3);
            let arity = proof.u32(list);
            let n = proof.u32(next.clone());
            let size = proof.compare(Operation::Eq, n, arity);
            let distinct = call(
                &mut proof,
                "operation-distinct-fields",
                vec![input(0), next],
                SemanticType::Bool,
            );
            proof.and(size, distinct)
        } else {
            let length = proof.u32(next);
            let zero = proof.n(0);
            proof.compare(Operation::Gt, length, zero)
        };
        let valid = all(&mut proof, vec![nonempty, same, extra]);
        graphs.push(proof.finish(vec![valid]));
        let mut guard = G::new(
            &format!("operation-types-{tag}"),
            signature(),
            vec![SemanticType::Bool],
        );
        let output = type_at(&mut guard, 4, 0);
        let valid_type = kind(&mut guard, output, if tag == 38 { 12 } else { 13 });
        let valid = choose(
            &mut guard,
            valid_type,
            &format!("operation-named-proof-{tag}"),
            "operation-invalid",
            vec![input(0), input(1)],
            SemanticType::Bool,
        );
        graphs.push(guard.finish(vec![valid]));
    }
    for tag in [39, 41] {
        let mut g = G::new(
            &format!("operation-types-{tag}"),
            signature(),
            vec![SemanticType::Bool],
        );
        let a = type_at(&mut g, 3, 0);
        let a = kind(&mut g, a, if tag == 39 { 12 } else { 13 });
        let op = operation_at(&mut g);
        let name = g.advance(op, 1);
        let len = g.u32(name);
        let zero = g.n(0);
        let nonempty = g.compare(Operation::Gt, len, zero);
        let mut flags = vec![a, nonempty];
        if tag == 41 {
            let output = type_at(&mut g, 4, 0);
            flags.push(kind(&mut g, output, 14));
        }
        let valid = all(&mut g, flags);
        graphs.push(g.finish(vec![valid]));
    }
    graphs
}
fn dispatch(graphs: &mut Vec<Graph>, low: u8, high: u8) -> String {
    if low == high {
        return if matches!(low, 0..=3 | 17..=46 | 48 | 66..=73) {
            format!("operation-check-{low}")
        } else {
            "operation-invalid".into()
        };
    }
    let middle = low + (high - low) / 2;
    let left = dispatch(graphs, low, middle);
    let right = dispatch(graphs, middle + 1, high);
    let name = format!("operation-dispatch-{low}-{high}");
    let mut g = G::new(&name, signature(), vec![SemanticType::Bool]);
    let op = operation_at(&mut g);
    let tag = g.byte(op);
    let boundary = g.n(middle as i128);
    let test = g.compare(Operation::Le, tag, boundary);
    let result = choose(
        &mut g,
        test,
        &left,
        &right,
        vec![input(0), input(1)],
        SemanticType::Bool,
    );
    graphs.push(g.finish(vec![result]));
    name
}
pub(super) fn graphs() -> Vec<Graph> {
    let mut invalid = G::new("operation-invalid", signature(), vec![SemanticType::Bool]);
    let no = invalid.bool(false);
    let mut graphs = vec![invalid.finish(vec![no])];
    graphs.extend(constant_graphs());
    graphs.extend(arithmetic_graphs());
    graphs.extend(composite_graphs());
    graphs.extend(extended_arithmetic_graphs());
    graphs.extend(truncate_graphs());
    graphs.extend(named_graphs());
    for tag in 22..=25 {
        graphs.push(boolean_graph(tag, if tag == 25 { 1 } else { 2 }));
    }
    for (tag, count) in [
        (0, 0),
        (1, 2),
        (2, 2),
        (3, 2),
        (17, 2),
        (18, 2),
        (19, 2),
        (20, 2),
        (21, 2),
        (22, 2),
        (23, 2),
        (24, 2),
        (25, 1),
        (26, 2),
        (27, 2),
        (28, 1),
        (29, 1),
        (70, 2),
        (71, 2),
        (72, 2),
        (73, 1),
        (30, usize::MAX),
        (31, 2),
        (32, 1),
        (33, 2),
        (34, 2),
        (35, 1),
        (36, 1),
        (37, 1),
        (38, usize::MAX),
        (39, 1),
        (40, 1),
        (41, 1),
        (42, 1),
        (43, 0),
        (44, 1),
        (45, 1),
        (46, 2),
        (48, 2),
        (66, 3),
        (67, 2),
        (68, 1),
        (69, 1),
    ] {
        let mut g = G::new(
            &format!("operation-check-{tag}"),
            signature(),
            vec![SemanticType::Bool],
        );
        let test = fixed_shape(
            &mut g,
            if count == usize::MAX {
                None
            } else {
                Some(count)
            },
        );
        let result = choose(
            &mut g,
            test,
            &format!("operation-types-{tag}"),
            "operation-invalid",
            vec![input(0), input(1)],
            SemanticType::Bool,
        );
        graphs.push(g.finish(vec![result]));
    }
    let entry = dispatch(&mut graphs, 0, 73);
    let mut main = G::new(
        "validator-node-operation",
        signature(),
        vec![SemanticType::Bool],
    );
    let e = main.field(input(1), 5);
    let c = main.field(input(1), 6);
    let en = main.u32(e);
    let cn = main.u32(c);
    let pure = equal(&mut main, en, 0);
    let caps = equal(&mut main, cn, 0);
    let pure = main.and(pure, caps);
    let operation = operation_at(&mut main);
    let tag = main.byte(operation);
    let last = main.n(73);
    let known = main.compare(Operation::Le, tag, last);
    let pure = main.and(pure, known);
    let valid = choose(
        &mut main,
        pure,
        &entry,
        "operation-invalid",
        vec![input(0), input(1)],
        SemanticType::Bool,
    );
    graphs.push(main.finish(vec![valid]));
    graphs
}
