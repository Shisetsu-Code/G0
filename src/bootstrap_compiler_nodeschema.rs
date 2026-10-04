//! Named operation proofs over schema and program offset tables, in G0.
//! Registry validity, named type closure and global purity are separate gates.
use super::*;
fn seq() -> SemanticType {
    SemanticType::Slice(Box::new(int()))
}
fn rows() -> SemanticType {
    SemanticType::Slice(Box::new(seq()))
}
fn signature() -> Vec<SemanticType> {
    vec![SemanticType::Bytes, rows(), rows(), seq()]
}
fn args(g: &G, values: Vec<SourceEndpoint>) -> Vec<(SourceEndpoint, SemanticType)> {
    values
        .into_iter()
        .map(|v| {
            let mut ty = g.ty(&v);
            if matches!(&ty,SemanticType::Integer(r)if r.min>=0&&r.max<=1_i128<<48) {
                ty = int();
            }
            (v, ty)
        })
        .collect()
}
fn call(g: &mut G, name: &str, values: Vec<SourceEndpoint>, ty: SemanticType) -> SourceEndpoint {
    let values = args(g, values);
    g.op(Operation::Subgraph(name.into()), values, ty)
}
fn choose(
    g: &mut G,
    test: SourceEndpoint,
    yes: &str,
    no: &str,
    values: Vec<SourceEndpoint>,
    ty: SemanticType,
) -> SourceEndpoint {
    let mut values = args(g, values);
    values.insert(0, (test, SemanticType::Bool));
    let result = g.op(
        Operation::Select {
            when_true: yes.into(),
            when_false: no.into(),
        },
        values,
        ty,
    );
    let node = g.b.graph.nodes.last_mut().unwrap();
    node.inputs[0].name = "selector".into();
    for (i, p) in node.inputs[1..].iter_mut().enumerate() {
        p.name = format!("p{i}");
    }
    result
}
fn eq(g: &mut G, value: SourceEndpoint, n: i128) -> SourceEndpoint {
    let n = g.n(n);
    g.compare(Operation::Eq, value, n)
}
fn common() -> Vec<SourceEndpoint> {
    (0..4).map(input).collect()
}
fn operation(g: &mut G) -> SourceEndpoint {
    g.field(input(3), 2)
}
fn ty(g: &mut G, field: i128, ordinal: i128) -> SourceEndpoint {
    let list = g.field(input(3), field);
    let ordinal = g.n(ordinal);
    call(
        g,
        "validator-port-type-at",
        vec![input(0), list, ordinal],
        int(),
    )
}
fn lookup(g: &mut G, table: SourceEndpoint, name: SourceEndpoint) -> SourceEndpoint {
    call(
        g,
        "validator-graph-name-index",
        vec![input(0), table, name],
        int(),
    )
}
fn assign(g: &mut G, source: SourceEndpoint, target: SourceEndpoint) -> SourceEndpoint {
    call(
        g,
        "validator-type-assignable",
        vec![input(0), source, target],
        SemanticType::Bool,
    )
}
fn empty(g: &mut G, t: SemanticType) -> SourceEndpoint {
    g.op(Operation::MakeArray, vec![], t)
}
fn append(
    g: &mut G,
    current: SourceEndpoint,
    value: SourceEndpoint,
    t: SemanticType,
) -> SourceEndpoint {
    let item = g.op(
        Operation::MakeArray,
        vec![(
            value,
            match &t {
                SemanticType::Slice(inner) => *inner.clone(),
                _ => unreachable!(),
            },
        )],
        t.clone(),
    );
    g.op(
        Operation::ArrayConcat,
        vec![(current, t.clone()), (item, t.clone())],
        t,
    )
}
fn false_graph(name: &str, s: Vec<SemanticType>) -> Graph {
    let mut g = G::new(name, s, vec![SemanticType::Bool]);
    let no = g.bool(false);
    g.finish(vec![no])
}

fn field_lookup_graphs() -> Vec<Graph> {
    let a = vec![SemanticType::Bytes, rows(), int(), int()];
    let mut indexed = a.clone();
    indexed.push(int());
    let mut field_args = indexed.clone();
    field_args.extend([rows(), int()]);
    let mut invalid = G::new("nodeschema-lookup-missing", indexed.clone(), vec![seq()]);
    let e = empty(&mut invalid, seq());
    let mut missing_field = G::new("nodeschema-field-missing", field_args.clone(), vec![seq()]);
    let e_field = empty(&mut missing_field, seq());
    let mut found = G::new("nodeschema-field-found", field_args.clone(), vec![seq()]);
    let field = found.index(input(5), input(6));
    let mut schema = G::new("nodeschema-lookup-known", indexed.clone(), vec![seq()]);
    let row = schema.index(input(1), input(4));
    let fields_at = schema.field(row, 4);
    let fields = call(
        &mut schema,
        "reader-schema-fields",
        vec![input(0), fields_at],
        rows(),
    );
    let index = lookup(&mut schema, fields.clone(), input(3));
    let len = schema.length(fields.clone());
    let known = schema.compare(Operation::Lt, index.clone(), len);
    let result = choose(
        &mut schema,
        known,
        "nodeschema-field-found",
        "nodeschema-field-missing",
        vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            fields,
            index,
        ],
        seq(),
    );
    let mut main = G::new("nodeschema-lookup", a, vec![seq()]);
    let index = lookup(&mut main, input(1), input(2));
    let len = main.length(input(1));
    let known = main.compare(Operation::Lt, index.clone(), len);
    let result_main = choose(
        &mut main,
        known,
        "nodeschema-lookup-known",
        "nodeschema-lookup-missing",
        vec![input(0), input(1), input(2), input(3), index],
        seq(),
    );
    vec![
        invalid.finish(vec![e]),
        missing_field.finish(vec![e_field]),
        found.finish(vec![field]),
        schema.finish(vec![result]),
        main.finish(vec![result_main]),
    ]
}

fn record_graphs() -> Vec<Graph> {
    let list_state = vec![SemanticType::Bytes, int(), int(), rows()];
    let mut c = G::new(
        "nodeschema-record-names-condition",
        list_state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = c.n(0);
    let names_more = c.compare(Operation::Gt, input(2), zero);
    let mut b = G::new(
        "nodeschema-record-names-body",
        list_state.clone(),
        list_state.clone(),
    );
    let next = b.skip_blob(input(1));
    let zero = b.n(0);
    let row = b.op(
        Operation::MakeArray,
        vec![(zero.clone(), int()), (zero, int()), (input(1), int())],
        seq(),
    );
    let body_refs = append(&mut b, input(3), row, rows());
    let one = b.n(1);
    let left = b.arithmetic(Operation::Sub, input(2), one);
    let mut names = G::new(
        "nodeschema-record-names",
        vec![SemanticType::Bytes, int()],
        vec![rows()],
    );
    let count = names.u32(input(1));
    let at = names.advance(input(1), 4);
    let e = empty(&mut names, rows());
    let out = names.loop_node(
        "nodeschema-record-names",
        list_state,
        vec![input(0), at, count, e],
    );
    // Input field types are assignable to their registered types.
    let state = vec![
        SemanticType::Bytes,
        rows(),
        rows(),
        int(),
        int(),
        SemanticType::Bool,
    ];
    let mut fc = G::new(
        "nodeschema-record-values-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = fc.length(input(2));
    let more = fc.compare(Operation::Lt, input(4), len);
    let more_values = fc.and(more, input(5));
    let mut extended = state.clone();
    extended.push(int());
    let mut proof = G::new(
        "nodeschema-record-value-found",
        extended.clone(),
        vec![SemanticType::Bool],
    );
    let field = proof.index(input(1), input(6));
    let target = proof.field(field, 3);
    let source = call(
        &mut proof,
        "validator-port-type-at",
        vec![input(0), input(3), input(4)],
        int(),
    );
    let valid_value = assign(&mut proof, source, target);
    let mut fb = G::new(
        "nodeschema-record-values-body",
        state.clone(),
        state.clone(),
    );
    let row = fb.index(input(2), input(4));
    let name = fb.field(row, 2);
    let index = lookup(&mut fb, input(1), name);
    let len = fb.length(input(1));
    let exists = fb.compare(Operation::Lt, index.clone(), len);
    let value = choose(
        &mut fb,
        exists,
        "nodeschema-record-value-found",
        "nodeschema-record-value-missing",
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
    let valid_values = fb.and(input(5), value);
    let next_values = fb.advance(input(4), 1);
    let rs = vec![
        SemanticType::Bytes,
        rows(),
        rows(),
        int(),
        SemanticType::Bool,
    ];
    let mut rc = G::new(
        "nodeschema-record-required-condition",
        rs.clone(),
        vec![SemanticType::Bool],
    );
    let len = rc.length(input(1));
    let more = rc.compare(Operation::Lt, input(3), len);
    let required_more = rc.and(more, input(4));
    let mut rb = G::new("nodeschema-record-required-body", rs.clone(), rs.clone());
    let field = rb.index(input(1), input(3));
    let at = rb.field(field.clone(), 4);
    let requirement = rb.byte(at);
    let required = eq(&mut rb, requirement, 0);
    let optional = rb.not(required);
    let name = rb.field(field, 2);
    let index = lookup(&mut rb, input(2), name);
    let len = rb.length(input(2));
    let present = rb.compare(Operation::Lt, index, len);
    let valid = rb.logic(Operation::Or, optional, present);
    let required_valid = rb.and(input(4), valid);
    let required_next = rb.advance(input(3), 1);
    let mut known_args = signature();
    known_args.push(int());
    let mut known = G::new(
        "nodeschema-record-known",
        known_args.clone(),
        vec![SemanticType::Bool],
    );
    let schema = known.index(input(1), input(4));
    let fields_at = known.field(schema, 4);
    let fields = call(
        &mut known,
        "reader-schema-fields",
        vec![input(0), fields_at],
        rows(),
    );
    let operation_at = operation(&mut known);
    let schema_at = known.advance(operation_at, 1);
    let names_at = known.skip_blob(schema_at);
    let refs = call(
        &mut known,
        "nodeschema-record-names",
        vec![input(0), names_at],
        rows(),
    );
    let ports = known.field(input(3), 3);
    let zero = known.n(0);
    let yes = known.bool(true);
    let values = known.loop_node(
        "nodeschema-record-values",
        state,
        vec![
            input(0),
            fields.clone(),
            refs.clone(),
            ports,
            zero.clone(),
            yes.clone(),
        ],
    );
    let required = known.loop_node(
        "nodeschema-record-required",
        rs,
        vec![input(0), fields, refs, zero, yes],
    );
    let record_valid = known.and(values[5].clone(), required[4].clone());
    let mut main = G::new("nodeschema-record", signature(), vec![SemanticType::Bool]);
    let op = operation(&mut main);
    let name = main.advance(op, 1);
    let index = lookup(&mut main, input(1), name);
    let len = main.length(input(1));
    let exists = main.compare(Operation::Lt, index.clone(), len);
    let mut args = common();
    args.push(index);
    let record = choose(
        &mut main,
        exists,
        "nodeschema-record-known",
        "nodeschema-record-missing",
        args,
        SemanticType::Bool,
    );
    vec![
        c.finish(vec![names_more]),
        b.finish(vec![input(0), next, left, body_refs]),
        names.finish(vec![out[3].clone()]),
        fc.finish(vec![more_values]),
        proof.finish(vec![valid_value]),
        false_graph("nodeschema-record-value-missing", extended),
        fb.finish(vec![
            input(0),
            input(1),
            input(2),
            input(3),
            next_values,
            valid_values,
        ]),
        rc.finish(vec![required_more]),
        rb.finish(vec![
            input(0),
            input(1),
            input(2),
            required_next,
            required_valid,
        ]),
        known.finish(vec![record_valid]),
        false_graph("nodeschema-record-missing", known_args),
        main.finish(vec![record]),
    ]
}

fn single_graphs() -> Vec<Graph> {
    let mut extended = signature();
    extended.push(seq());
    let mut graphs = vec![false_graph("nodeschema-single-missing", extended.clone())];
    let mut optional = G::new(
        "nodeschema-field-optional-proof",
        extended.clone(),
        vec![SemanticType::Bool],
    );
    let source = optional.field(input(4), 3);
    let target = ty(&mut optional, 4, 0);
    let target = optional.advance(target, 1);
    let assigned = assign(&mut optional, source, target);
    graphs.push(optional.finish(vec![assigned]));
    let mut optional = G::new(
        "nodeschema-field-optional",
        extended.clone(),
        vec![SemanticType::Bool],
    );
    let target = ty(&mut optional, 4, 0);
    let tag = optional.byte(target);
    let option = eq(&mut optional, tag, 14);
    let mut a = common();
    a.push(input(4));
    let valid_optional = choose(
        &mut optional,
        option,
        "nodeschema-field-optional-proof",
        "nodeschema-single-missing",
        a,
        SemanticType::Bool,
    );
    graphs.push(optional.finish(vec![valid_optional]));
    let mut required = G::new(
        "nodeschema-field-required",
        extended.clone(),
        vec![SemanticType::Bool],
    );
    let source = required.field(input(4), 3);
    let target = ty(&mut required, 4, 0);
    let valid_required = assign(&mut required, source, target);
    graphs.push(required.finish(vec![valid_required]));
    let mut field = G::new(
        "nodeschema-field-proof",
        extended.clone(),
        vec![SemanticType::Bool],
    );
    let requirement = field.field(input(4), 4);
    let requirement = field.byte(requirement);
    let is_optional = eq(&mut field, requirement, 1);
    let mut a = common();
    a.push(input(4));
    let field_valid = choose(
        &mut field,
        is_optional,
        "nodeschema-field-optional",
        "nodeschema-field-required",
        a,
        SemanticType::Bool,
    );
    graphs.push(field.finish(vec![field_valid]));
    let mut variant = G::new(
        "nodeschema-variant-proof",
        extended.clone(),
        vec![SemanticType::Bool],
    );
    let source = ty(&mut variant, 3, 0);
    let target = variant.field(input(4), 3);
    let variant_valid = assign(&mut variant, source, target);
    graphs.push(variant.finish(vec![variant_valid]));
    let mut payload = G::new(
        "nodeschema-payload-proof",
        extended,
        vec![SemanticType::Bool],
    );
    let source = payload.field(input(4), 3);
    let target = ty(&mut payload, 4, 0);
    let target = payload.advance(target, 1);
    let payload_valid = assign(&mut payload, source, target);
    graphs.push(payload.finish(vec![payload_valid]));
    for (name, proof, tag) in [
        ("nodeschema-field", "nodeschema-field-proof", 39),
        ("nodeschema-variant", "nodeschema-variant-proof", 40),
        ("nodeschema-payload", "nodeschema-payload-proof", 41),
    ] {
        let mut g = G::new(name, signature(), vec![SemanticType::Bool]);
        let op = operation(&mut g);
        let first = g.advance(op, 1);
        let (schema_name, field_name) = if tag == 40 {
            let name = g.skip_blob(first.clone());
            (first, name)
        } else {
            let input_type = ty(&mut g, 3, 0);
            let schema = g.advance(input_type, 1);
            (schema, first)
        };
        let info = call(
            &mut g,
            "nodeschema-lookup",
            vec![input(0), input(1), schema_name, field_name],
            seq(),
        );
        let len = g.length(info.clone());
        let zero = g.n(0);
        let found = g.compare(Operation::Gt, len, zero);
        let mut a = common();
        a.push(info);
        let valid = choose(
            &mut g,
            found,
            proof,
            "nodeschema-single-missing",
            a,
            SemanticType::Bool,
        );
        graphs.push(g.finish(vec![valid]));
    }
    graphs
}

fn match_graphs() -> Vec<Graph> {
    let s = vec![SemanticType::Bytes, int(), int(), rows()];
    let mut c = G::new(
        "nodeschema-match-list-condition",
        s.clone(),
        vec![SemanticType::Bool],
    );
    let z = c.n(0);
    let list_more = c.compare(Operation::Gt, input(2), z);
    let mut b = G::new("nodeschema-match-list-body", s.clone(), s.clone());
    let callee = b.skip_blob(input(1));
    let next = b.skip_blob(callee.clone());
    let z = b.n(0);
    let row = b.op(
        Operation::MakeArray,
        vec![
            (z.clone(), int()),
            (z, int()),
            (input(1), int()),
            (callee, int()),
        ],
        seq(),
    );
    let match_body_refs = append(&mut b, input(3), row, rows());
    let one = b.n(1);
    let left = b.arithmetic(Operation::Sub, input(2), one);
    let mut list = G::new(
        "nodeschema-match-list",
        vec![SemanticType::Bytes, int()],
        vec![rows()],
    );
    let count = list.u32(input(1));
    let start = list.advance(input(1), 4);
    let e = empty(&mut list, rows());
    let out = list.loop_node("nodeschema-match-list", s, vec![input(0), start, count, e]);
    let mut proof_state = signature();
    proof_state.extend([rows(), int(), SemanticType::Bool]);
    let mut pc = G::new(
        "nodeschema-match-proof-condition",
        proof_state.clone(),
        vec![SemanticType::Bool],
    );
    let len = pc.length(input(4));
    let more = pc.compare(Operation::Lt, input(5), len);
    let proof_more = pc.and(more, input(6));
    let mut pb = G::new(
        "nodeschema-match-proof-body",
        proof_state.clone(),
        proof_state.clone(),
    );
    let row = pb.index(input(4), input(5));
    let tag = pb.field(row.clone(), 2);
    let callee = pb.field(row, 3);
    let len = pb.u32(tag.clone());
    let zero = pb.n(0);
    let nonempty = pb.compare(Operation::Gt, len, zero);
    let count = call(
        &mut pb,
        "validator-name-count",
        vec![input(0), input(4), tag],
        int(),
    );
    let unique = eq(&mut pb, count, 1);
    let one = pb.n(1);
    let yes = pb.bool(true);
    let target = call(
        &mut pb,
        "controlcheck-target",
        vec![input(0), input(2), input(3), callee, one, yes],
        SemanticType::Bool,
    );
    let valid = pb.and(nonempty, unique);
    let valid = pb.and(valid, target);
    let proof_valid = pb.and(input(6), valid);
    let next_index = pb.advance(input(5), 1);
    let mut shape = G::new(
        "nodeschema-match-variant",
        signature(),
        vec![SemanticType::Bool],
    );
    let input_type = ty(&mut shape, 3, 0);
    let schema = shape.advance(input_type, 1);
    let index = lookup(&mut shape, input(1), schema);
    let len = shape.length(input(1));
    let exists = shape.compare(Operation::Lt, index, len);
    let op = operation(&mut shape);
    let count_at = shape.advance(op, 1);
    let count = shape.u32(count_at.clone());
    let refs = call(
        &mut shape,
        "nodeschema-match-list",
        vec![input(0), count_at.clone()],
        rows(),
    );
    let start = shape.advance(count_at, 4);
    let empty_refs = empty(&mut shape, seq());
    let end = shape.loop_node(
        "callcycles-match",
        vec![SemanticType::Bytes, rows(), int(), int(), seq()],
        vec![input(0), input(2), start, count, empty_refs],
    );
    let one = shape.n(1);
    let yes = shape.bool(true);
    let default = call(
        &mut shape,
        "controlcheck-target",
        vec![
            input(0),
            input(2),
            input(3),
            end[2].clone(),
            one,
            yes.clone(),
        ],
        SemanticType::Bool,
    );
    let zero = shape.n(0);
    let mut a = common();
    a.extend([refs, zero, yes]);
    let arms = shape.loop_node("nodeschema-match-proof", proof_state, a);
    let valid = shape.and(exists, default);
    let match_valid = shape.and(valid, arms[6].clone());
    let mut guard = G::new(
        "nodeschema-match-selector",
        signature(),
        vec![SemanticType::Bool],
    );
    let selector = ty(&mut guard, 3, 0);
    let tag = guard.byte(selector);
    let variant = eq(&mut guard, tag, 13);
    let checked = choose(
        &mut guard,
        variant,
        "nodeschema-match-variant",
        "nodeschema-invalid",
        common(),
        SemanticType::Bool,
    );
    let mut main = G::new("nodeschema-match", signature(), vec![SemanticType::Bool]);
    let ins = main.field(input(3), 3);
    let count = main.u32(ins);
    let zero = main.n(0);
    let present = main.compare(Operation::Gt, count, zero);
    let e = main.field(input(3), 5);
    let caps = main.field(input(3), 6);
    let e = main.u32(e);
    let caps = main.u32(caps);
    let pure = eq(&mut main, e, 0);
    let caps = eq(&mut main, caps, 0);
    let valid = main.and(present, pure);
    let valid = main.and(valid, caps);
    let checked_main = choose(
        &mut main,
        valid,
        "nodeschema-match-selector",
        "nodeschema-invalid",
        common(),
        SemanticType::Bool,
    );
    vec![
        c.finish(vec![list_more]),
        b.finish(vec![input(0), next, left, match_body_refs]),
        list.finish(vec![out[3].clone()]),
        pc.finish(vec![proof_more]),
        pb.finish(vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            next_index,
            proof_valid,
        ]),
        shape.finish(vec![match_valid]),
        guard.finish(vec![checked]),
        main.finish(vec![checked_main]),
    ]
}

pub(super) fn graphs() -> Vec<Graph> {
    let mut graphs = field_lookup_graphs();
    graphs.extend(record_graphs());
    graphs.extend(single_graphs());
    graphs.extend(match_graphs());
    graphs.push(false_graph("nodeschema-invalid", signature()));
    let mut passthrough = G::new("nodeschema-other", signature(), vec![SemanticType::Bool]);
    let yes = passthrough.bool(true);
    graphs.push(passthrough.finish(vec![yes]));
    for (name, tag, yes, no) in [
        (
            "nodeschema-dispatch-record",
            38,
            "nodeschema-record",
            "nodeschema-dispatch-field",
        ),
        (
            "nodeschema-dispatch-field",
            39,
            "nodeschema-field",
            "nodeschema-dispatch-variant",
        ),
        (
            "nodeschema-dispatch-variant",
            40,
            "nodeschema-variant",
            "nodeschema-payload",
        ),
    ] {
        let mut g = G::new(name, signature(), vec![SemanticType::Bool]);
        let op = operation(&mut g);
        let tag_value = g.byte(op);
        let test = eq(&mut g, tag_value, tag);
        let result = choose(&mut g, test, yes, no, common(), SemanticType::Bool);
        graphs.push(g.finish(vec![result]));
    }
    let mut pure = G::new(
        "nodeschema-operation-shape",
        signature(),
        vec![SemanticType::Bool],
    );
    let valid = call(
        &mut pure,
        "validator-node-operation",
        vec![input(0), input(3)],
        SemanticType::Bool,
    );
    let result = choose(
        &mut pure,
        valid,
        "nodeschema-dispatch-record",
        "nodeschema-invalid",
        common(),
        SemanticType::Bool,
    );
    graphs.push(pure.finish(vec![result]));
    let mut gate = G::new(
        "nodeschema-named-gate",
        signature(),
        vec![SemanticType::Bool],
    );
    let op = operation(&mut gate);
    let tag = gate.byte(op);
    let low = gate.n(38);
    let high = gate.n(41);
    let after = gate.compare(Operation::Ge, tag.clone(), low);
    let before = gate.compare(Operation::Le, tag, high);
    let named = gate.and(after, before);
    let result = choose(
        &mut gate,
        named,
        "nodeschema-operation-shape",
        "nodeschema-other",
        common(),
        SemanticType::Bool,
    );
    graphs.push(gate.finish(vec![result]));
    let mut main = G::new(
        "validator-node-schema",
        signature(),
        vec![SemanticType::Bool],
    );
    let op = operation(&mut main);
    let tag = main.byte(op);
    let matched = eq(&mut main, tag, 5);
    let result = choose(
        &mut main,
        matched,
        "nodeschema-match",
        "nodeschema-named-gate",
        common(),
        SemanticType::Bool,
    );
    graphs.push(main.finish(vec![result]));
    let fast = super::control_fast::variants(
        &graphs,
        &[("controlcheck-target", "controlcheck-target-fast")],
    );
    graphs.extend(fast);
    graphs
}
