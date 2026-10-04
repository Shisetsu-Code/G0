//! Schema registry offset tables and validation implemented in G0.
use super::*;
fn append_row(g: &mut G, existing: SourceEndpoint, fields: Vec<SourceEndpoint>) -> SourceEndpoint {
    let row = g.op(
        Operation::MakeArray,
        fields.into_iter().map(|e| (e, int())).collect(),
        SemanticType::Slice(Box::new(int())),
    );
    let item = g.op(
        Operation::MakeArray,
        vec![(row, SemanticType::Slice(Box::new(int())))],
        rows(),
    );
    g.op(
        Operation::ArrayConcat,
        vec![(existing, rows()), (item, rows())],
        rows(),
    )
}
fn table_graphs() -> Vec<Graph> {
    let mut offset = G::new(
        "reader-program-schema-offset",
        vec![SemanticType::Bytes],
        vec![int()],
    );
    let eight = offset.n(8);
    let at = offset.skip_blob(eight);
    let count = offset.u32(at.clone());
    let start = offset.advance(at, 4);
    let end = offset.loop_node(
        "validator-schema-skip",
        vec![SemanticType::Bytes, int(), int()],
        vec![input(0), start, count],
    );
    let state = vec![SemanticType::Bytes, int(), int(), rows()];
    let mut sc = G::new(
        "reader-schema-table-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = sc.n(0);
    let schema_more = sc.compare(Operation::Gt, input(2), zero);
    let mut sb = G::new("reader-schema-table-body", state.clone(), state.clone());
    let version = sb.skip_blob(input(1));
    let fields_at = sb.advance(version.clone(), 4);
    let next = sb.call("reader-schema", input(1));
    let zero = sb.n(0);
    let schemas = append_row(
        &mut sb,
        input(3),
        vec![
            input(1),
            next.clone(),
            input(1),
            version,
            fields_at,
            zero.clone(),
            zero,
        ],
    );
    let one = sb.n(1);
    let schema_left = sb.arithmetic(Operation::Sub, input(2), one);
    let mut sm = G::new(
        "reader-program-schemas",
        vec![SemanticType::Bytes],
        vec![rows()],
    );
    let at = call(
        &mut sm,
        "reader-program-schema-offset",
        vec![input(0)],
        int(),
    );
    let count = sm.u32(at.clone());
    let start = sm.advance(at, 4);
    let empty = sm.op(Operation::MakeArray, vec![], rows());
    let schemas_result = sm.loop_node(
        "reader-schema-table",
        state.clone(),
        vec![input(0), start, count, empty],
    );
    let mut fc = G::new(
        "reader-schema-fields-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = fc.n(0);
    let field_more = fc.compare(Operation::Gt, input(2), zero);
    let mut fb = G::new("reader-schema-fields-body", state.clone(), state.clone());
    let tag = fb.u32(input(1));
    let name = fb.advance(input(1), 4);
    let type_word = fb.skip_blob(name.clone());
    let type_at = fb.advance(type_word.clone(), 4);
    let requirement = fb.skip_blob(type_word.clone());
    let field_next = fb.advance(requirement.clone(), 1);
    let fields = append_row(
        &mut fb,
        input(3),
        vec![
            tag,
            field_next.clone(),
            name,
            type_at,
            requirement,
            input(1),
            type_word,
        ],
    );
    let one = fb.n(1);
    let field_left = fb.arithmetic(Operation::Sub, input(2), one);
    let mut fm = G::new(
        "reader-schema-fields",
        vec![SemanticType::Bytes, int()],
        vec![rows()],
    );
    let count = fm.u32(input(1));
    let start = fm.advance(input(1), 4);
    let empty = fm.op(Operation::MakeArray, vec![], rows());
    let field_result = fm.loop_node(
        "reader-schema-fields",
        state,
        vec![input(0), start, count, empty],
    );
    vec![
        offset.finish(vec![end[1].clone()]),
        sc.finish(vec![schema_more]),
        sb.finish(vec![input(0), next, schema_left, schemas]),
        sm.finish(vec![schemas_result[3].clone()]),
        fc.finish(vec![field_more]),
        fb.finish(vec![input(0), field_next, field_left, fields]),
        fm.finish(vec![field_result[3].clone()]),
    ]
}
fn registry_graphs() -> Vec<Graph> {
    let state = vec![
        SemanticType::Bytes,
        rows(),
        int(),
        SemanticType::Bool,
        int(),
    ];
    let mut fc = G::new(
        "validator-schema-fields-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = fc.length(input(1));
    let more = fc.compare(Operation::Lt, input(2), len);
    let field_more = fc.and(input(3), more);
    let mut fb = G::new("validator-schema-fields-body", state.clone(), state.clone());
    let row = fb.index(input(1), input(2));
    let tag = fb.field(row.clone(), 0);
    let name = fb.field(row, 2);
    let fresh = fb.compare(Operation::Gt, tag.clone(), input(4));
    let name_len = fb.u32(name.clone());
    let zero = fb.n(0);
    let nonempty = fb.compare(Operation::Gt, name_len, zero);
    let count = call(
        &mut fb,
        "validator-name-count",
        vec![input(0), input(1), name],
        int(),
    );
    let one = fb.n(1);
    let unique = fb.compare(Operation::Eq, count, one.clone());
    let valid = fb.and(input(3), fresh);
    let valid = fb.and(valid, nonempty);
    let field_valid = fb.and(valid, unique);
    let field_next = fb.arithmetic(Operation::Add, input(2), one);
    let mut fm = G::new(
        "validator-schema-fields",
        vec![SemanticType::Bytes, int()],
        vec![SemanticType::Bool],
    );
    let fields = call(
        &mut fm,
        "reader-schema-fields",
        vec![input(0), input(1)],
        rows(),
    );
    let zero = fm.n(0);
    let yes = fm.bool(true);
    let result = fm.loop_node(
        "validator-schema-fields",
        state,
        vec![input(0), fields, zero.clone(), yes, zero],
    );
    let state = vec![SemanticType::Bytes, rows(), int(), SemanticType::Bool];
    let mut sc = G::new(
        "validator-schemas-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = sc.length(input(1));
    let more = sc.compare(Operation::Lt, input(2), len);
    let more = sc.and(input(3), more);
    let mut sb = G::new("validator-schemas-body", state.clone(), state.clone());
    let row = sb.index(input(1), input(2));
    let name = sb.field(row.clone(), 2);
    let version = sb.field(row.clone(), 3);
    let fields = sb.field(row, 4);
    let version = sb.u32(version);
    let zero = sb.n(0);
    let version_valid = sb.compare(Operation::Gt, version, zero.clone());
    let len = sb.u32(name.clone());
    let nonempty = sb.compare(Operation::Gt, len, zero);
    let count = call(
        &mut sb,
        "validator-name-count",
        vec![input(0), input(1), name],
        int(),
    );
    let one = sb.n(1);
    let unique = sb.compare(Operation::Eq, count, one.clone());
    let fields_valid = call(
        &mut sb,
        "validator-schema-fields",
        vec![input(0), fields],
        SemanticType::Bool,
    );
    let valid = sb.and(input(3), version_valid);
    let valid = sb.and(valid, nonempty);
    let valid = sb.and(valid, unique);
    let valid = sb.and(valid, fields_valid);
    let next = sb.arithmetic(Operation::Add, input(2), one);
    let mut sm = G::new(
        "validator-program-schemas",
        vec![SemanticType::Bytes],
        vec![SemanticType::Bool],
    );
    let schemas = call(&mut sm, "reader-program-schemas", vec![input(0)], rows());
    let zero = sm.n(0);
    let yes = sm.bool(true);
    let result_schemas = sm.loop_node(
        "validator-schemas",
        state,
        vec![input(0), schemas, zero, yes],
    );
    vec![
        fc.finish(vec![field_more]),
        fb.finish(vec![input(0), input(1), field_next, field_valid, tag]),
        fm.finish(vec![result[3].clone()]),
        sc.finish(vec![more]),
        sb.finish(vec![input(0), input(1), next, valid]),
        sm.finish(vec![result_schemas[3].clone()]),
    ]
}
pub(super) fn graphs() -> Vec<Graph> {
    let mut graphs = table_graphs();
    graphs.extend(registry_graphs());
    graphs.extend(type_graphs());
    graphs.extend(port_graphs());
    graphs
}
fn port_graphs() -> Vec<Graph> {
    let ports_state = vec![
        SemanticType::Bytes,
        rows(),
        int(),
        int(),
        SemanticType::Bool,
    ];
    let mut pc = G::new(
        "validator-schema-ports-condition",
        ports_state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = pc.n(0);
    let ports_more = pc.compare(Operation::Gt, input(3), zero);
    let ports_more = pc.and(ports_more, input(4));
    let mut pb = G::new(
        "validator-schema-ports-body",
        ports_state.clone(),
        ports_state.clone(),
    );
    let name = pb.advance(input(2), 2);
    let ty = pb.skip_blob(name);
    let valid = call(
        &mut pb,
        "validator-schema-type",
        vec![input(0), input(1), ty],
        SemanticType::Bool,
    );
    let ports_valid = pb.and(input(4), valid);
    let ports_next = pb.call("reader-port-layout", input(2));
    let one = pb.n(1);
    let ports_left = pb.arithmetic(Operation::Sub, input(3), one);
    let mut pm = G::new(
        "validator-schema-ports",
        vec![SemanticType::Bytes, rows(), int()],
        vec![SemanticType::Bool],
    );
    let count = pm.u32(input(2));
    let start = pm.advance(input(2), 4);
    let yes = pm.bool(true);
    let ports_out = pm.loop_node(
        "validator-schema-ports",
        ports_state,
        vec![input(0), input(1), start, count, yes],
    );
    let nodes_state = vec![
        SemanticType::Bytes,
        rows(),
        rows(),
        rows(),
        int(),
        SemanticType::Bool,
    ];
    let mut nc = G::new(
        "validator-schema-node-types-condition",
        nodes_state.clone(),
        vec![SemanticType::Bool],
    );
    let len = nc.length(input(3));
    let nodes_more = nc.compare(Operation::Lt, input(4), len);
    let nodes_more = nc.and(nodes_more, input(5));
    let mut nb = G::new(
        "validator-schema-node-types-body",
        nodes_state.clone(),
        nodes_state.clone(),
    );
    let row = nb.index(input(3), input(4));
    let inputs = nb.field(row.clone(), 3);
    let outputs = nb.field(row.clone(), 4);
    let a = call(
        &mut nb,
        "validator-schema-ports",
        vec![input(0), input(1), inputs],
        SemanticType::Bool,
    );
    let b = call(
        &mut nb,
        "validator-schema-ports",
        vec![input(0), input(1), outputs],
        SemanticType::Bool,
    );
    let nodes_valid = nb.and(a, b);
    let schema_node = call(
        &mut nb,
        "validator-node-schema-fast",
        vec![input(0), input(1), input(2), row],
        SemanticType::Bool,
    );
    let nodes_valid = nb.and(nodes_valid, schema_node);
    let nodes_valid = nb.and(input(5), nodes_valid);
    let nodes_next = nb.advance(input(4), 1);
    let program_state = vec![
        SemanticType::Bytes,
        rows(),
        rows(),
        int(),
        SemanticType::Bool,
    ];
    let mut gc = G::new(
        "validator-schema-graph-types-condition",
        program_state.clone(),
        vec![SemanticType::Bool],
    );
    let len = gc.length(input(2));
    let graphs_more = gc.compare(Operation::Lt, input(3), len);
    let graphs_more = gc.and(graphs_more, input(4));
    let mut gb = G::new(
        "validator-schema-graph-types-body",
        program_state.clone(),
        program_state.clone(),
    );
    let row = gb.index(input(2), input(3));
    let start = gb.field(row.clone(), 0);
    let inputs = gb.field(row.clone(), 3);
    let outputs = gb.field(row, 4);
    let a = call(
        &mut gb,
        "validator-schema-ports",
        vec![input(0), input(1), inputs],
        SemanticType::Bool,
    );
    let b = call(
        &mut gb,
        "validator-schema-ports",
        vec![input(0), input(1), outputs],
        SemanticType::Bool,
    );
    let valid = gb.and(a, b);
    let nodes = call(&mut gb, "reader-graph-ast", vec![input(0), start], rows());
    let zero = gb.n(0);
    let yes = gb.bool(true);
    let nodes_out = gb.loop_node(
        "validator-schema-node-types",
        nodes_state,
        vec![input(0), input(1), input(2), nodes, zero, yes],
    );
    let graphs_valid = gb.and(valid, nodes_out[5].clone());
    let graphs_valid = gb.and(input(4), graphs_valid);
    let graphs_next = gb.advance(input(3), 1);
    let mut gm = G::new(
        "validator-program-port-types",
        vec![SemanticType::Bytes],
        vec![SemanticType::Bool],
    );
    let schemas = call(&mut gm, "reader-program-schemas", vec![input(0)], rows());
    let graphs = call(&mut gm, "reader-program-ast", vec![input(0)], rows());
    let zero = gm.n(0);
    let yes = gm.bool(true);
    let graphs_out = gm.loop_node(
        "validator-schema-graph-types",
        program_state,
        vec![input(0), schemas, graphs, zero, yes],
    );
    vec![
        pc.finish(vec![ports_more]),
        pb.finish(vec![
            input(0),
            input(1),
            ports_next,
            ports_left,
            ports_valid,
        ]),
        pm.finish(vec![ports_out[4].clone()]),
        nc.finish(vec![nodes_more]),
        nb.finish(vec![
            input(0),
            input(1),
            input(2),
            input(3),
            nodes_next,
            nodes_valid,
        ]),
        gc.finish(vec![graphs_more]),
        gb.finish(vec![
            input(0),
            input(1),
            input(2),
            graphs_next,
            graphs_valid,
        ]),
        gm.finish(vec![graphs_out[4].clone()]),
    ]
}
fn type_graphs() -> Vec<Graph> {
    let mut nb = G::new(
        "validator-schema-named-type",
        vec![SemanticType::Bytes, rows(), int()],
        vec![SemanticType::Bool],
    );
    let name = nb.advance(input(2), 1);
    let idx = call(
        &mut nb,
        "validator-graph-name-index",
        vec![input(0), input(1), name],
        int(),
    );
    let len = nb.length(input(1));
    let named_valid = nb.compare(Operation::Lt, idx, len);
    let mut ordinary = G::new(
        "validator-schema-ordinary-type",
        vec![SemanticType::Bytes, rows(), int()],
        vec![SemanticType::Bool],
    );
    let ordinary_valid = ordinary.bool(true);
    let fields_state = vec![
        SemanticType::Bytes,
        rows(),
        rows(),
        int(),
        SemanticType::Bool,
    ];
    let mut fc = G::new(
        "validator-schema-field-types-condition",
        fields_state.clone(),
        vec![SemanticType::Bool],
    );
    let len = fc.length(input(2));
    let fields_more = fc.compare(Operation::Lt, input(3), len);
    let fields_more = fc.and(fields_more, input(4));
    let mut fb = G::new(
        "validator-schema-field-types-body",
        fields_state.clone(),
        fields_state.clone(),
    );
    let row = fb.index(input(2), input(3));
    let at = fb.field(row, 3);
    let valid = call(
        &mut fb,
        "validator-schema-type",
        vec![input(0), input(1), at],
        SemanticType::Bool,
    );
    let fields_valid = fb.and(input(4), valid);
    let fields_next = fb.advance(input(3), 1);
    let mut fm = G::new(
        "validator-schema-field-types",
        vec![SemanticType::Bytes, rows(), int()],
        vec![SemanticType::Bool],
    );
    let fields = call(
        &mut fm,
        "reader-schema-fields",
        vec![input(0), input(2)],
        rows(),
    );
    let zero = fm.n(0);
    let yes = fm.bool(true);
    let fields_out = fm.loop_node(
        "validator-schema-field-types",
        fields_state,
        vec![input(0), input(1), fields, zero, yes],
    );
    let schemas_state = vec![SemanticType::Bytes, rows(), int(), SemanticType::Bool];
    let mut sc = G::new(
        "validator-schema-types-condition",
        schemas_state.clone(),
        vec![SemanticType::Bool],
    );
    let len = sc.length(input(1));
    let schemas_more = sc.compare(Operation::Lt, input(2), len);
    let schemas_more = sc.and(schemas_more, input(3));
    let mut sb = G::new(
        "validator-schema-types-body",
        schemas_state.clone(),
        schemas_state.clone(),
    );
    let row = sb.index(input(1), input(2));
    let at = sb.field(row, 4);
    let valid = call(
        &mut sb,
        "validator-schema-field-types",
        vec![input(0), input(1), at],
        SemanticType::Bool,
    );
    let schemas_valid = sb.and(input(3), valid);
    let schemas_next = sb.advance(input(2), 1);
    let mut sm = G::new(
        "validator-program-schema-types",
        vec![SemanticType::Bytes],
        vec![SemanticType::Bool],
    );
    let schemas = call(&mut sm, "reader-program-schemas", vec![input(0)], rows());
    let zero = sm.n(0);
    let yes = sm.bool(true);
    let schemas_out = sm.loop_node(
        "validator-schema-types",
        schemas_state,
        vec![input(0), schemas, zero, yes],
    );
    vec![
        nb.finish(vec![named_valid]),
        ordinary.finish(vec![ordinary_valid]),
        fc.finish(vec![fields_more]),
        fb.finish(vec![
            input(0),
            input(1),
            input(2),
            fields_next,
            fields_valid,
        ]),
        fm.finish(vec![fields_out[4].clone()]),
        sc.finish(vec![schemas_more]),
        sb.finish(vec![input(0), input(1), schemas_next, schemas_valid]),
        sm.finish(vec![schemas_out[3].clone()]),
    ]
}
