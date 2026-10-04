//! Emission caches and definition-only variants for proven canonical programs.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

// This transforms compiler graph DEFINITIONS at build time, never input GIR.
// Only dependency paths leading to an explicitly substituted helper are cloned.
pub(super) fn variants(graphs: &[Graph], replacements: &[(&str, &str)]) -> Vec<Graph> {
    fn names(operation: &Operation) -> Vec<&str> {
        match operation {
            Operation::Subgraph(name) | Operation::Map { body: name } => vec![name],
            Operation::Select {
                when_true,
                when_false,
            } => vec![when_true, when_false],
            Operation::Loop {
                condition, body, ..
            } => vec![condition, body],
            Operation::Match { arms, default } => arms
                .iter()
                .map(|arm| arm.graph.as_str())
                .chain(std::iter::once(default.as_str()))
                .collect(),
            _ => vec![],
        }
    }
    let mut affected = BTreeSet::new();
    loop {
        let before = affected.len();
        for graph in graphs {
            if graph
                .nodes
                .iter()
                .flat_map(|node| names(&node.operation))
                .any(|name| {
                    affected.contains(name) || replacements.iter().any(|(old, _)| *old == name)
                })
            {
                affected.insert(graph.name.clone());
            }
        }
        if affected.len() == before {
            break;
        }
    }
    let mut aliases: BTreeMap<String, String> = affected
        .iter()
        .map(|name| (name.clone(), format!("{name}-fast")))
        .collect();
    aliases.extend(
        replacements
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string())),
    );
    fn rename(name: &mut String, aliases: &BTreeMap<String, String>) {
        if let Some(new) = aliases.get(name) {
            *name = new.clone();
        }
    }
    graphs
        .iter()
        .filter(|graph| affected.contains(&graph.name))
        .map(|graph| {
            let mut graph = graph.clone();
            graph.name = aliases[&graph.name].clone();
            for node in &mut graph.nodes {
                match &mut node.operation {
                    Operation::Subgraph(name) | Operation::Map { body: name } => {
                        rename(name, &aliases)
                    }
                    Operation::Select {
                        when_true,
                        when_false,
                    } => {
                        rename(when_true, &aliases);
                        rename(when_false, &aliases);
                    }
                    Operation::Loop {
                        condition, body, ..
                    } => {
                        rename(condition, &aliases);
                        rename(body, &aliases);
                    }
                    Operation::Match { arms, default } => {
                        for arm in arms {
                            rename(&mut arm.graph, &aliases);
                        }
                        rename(default, &aliases);
                    }
                    _ => {}
                }
            }
            graph
        })
        .collect()
}

fn seq() -> SemanticType {
    SemanticType::Slice(Box::new(int()))
}
fn rows() -> SemanticType {
    SemanticType::Slice(Box::new(seq()))
}
fn texts() -> SemanticType {
    SemanticType::Slice(Box::new(SemanticType::Text))
}
fn args(g: &G, v: Vec<SourceEndpoint>) -> Vec<(SourceEndpoint, SemanticType)> {
    v.into_iter()
        .map(|v| {
            let mut t = g.ty(&v);
            if matches!(&t,SemanticType::Integer(r)if r.min>=0&&r.max<=1_i128<<48) {
                t = int();
            }
            (v, t)
        })
        .collect()
}
fn call(g: &mut G, n: &str, v: Vec<SourceEndpoint>, t: SemanticType) -> SourceEndpoint {
    let v = args(g, v);
    g.op(Operation::Subgraph(n.into()), v, t)
}
fn choose(
    g: &mut G,
    test: SourceEndpoint,
    yes: &str,
    no: &str,
    v: Vec<SourceEndpoint>,
    t: SemanticType,
) -> SourceEndpoint {
    let mut v = args(g, v);
    v.insert(0, (test, SemanticType::Bool));
    let out = g.op(
        Operation::Select {
            when_true: yes.into(),
            when_false: no.into(),
        },
        v,
        t,
    );
    let n = g.b.graph.nodes.last_mut().unwrap();
    n.inputs[0].name = "selector".into();
    for (i, p) in n.inputs[1..].iter_mut().enumerate() {
        p.name = format!("p{i}");
    }
    out
}
fn array(g: &mut G, v: Vec<SourceEndpoint>, t: SemanticType) -> SourceEndpoint {
    let v = args(g, v);
    g.op(Operation::MakeArray, v, t)
}
fn append(
    g: &mut G,
    current: SourceEndpoint,
    value: SourceEndpoint,
    t: SemanticType,
) -> SourceEndpoint {
    let one = array(g, vec![value], t.clone());
    g.op(
        Operation::ArrayConcat,
        vec![(current, t.clone()), (one, t.clone())],
        t,
    )
}
fn len(g: &mut G, v: SourceEndpoint) -> SourceEndpoint {
    let n = g.length(v);
    let t = g.ty(&n);
    g.op(Operation::ConvertChecked, vec![(n, t)], int())
}
fn eq(g: &mut G, v: SourceEndpoint, n: i128) -> SourceEndpoint {
    let n = g.n(n);
    g.compare(Operation::Eq, v, n)
}

fn edge_sort_graphs() -> Vec<Graph> {
    let mut graphs = vec![];
    let identity = G::new("control-fast-edges-identity", vec![rows()], vec![rows()]);
    graphs.push(identity.finish(vec![input(0)]));
    // Merge equal destinations from the left run first. The original source
    // order within each destination group is observable in emitted assembly.
    let state = vec![rows(), rows(), int(), int(), int(), int()];
    let prefix = "control-fast-edges-merge";
    let mut condition = G::new(
        &format!("{prefix}-condition"),
        state.clone(),
        vec![SemanticType::Bool],
    );
    let left = condition.compare(Operation::Lt, input(2), input(4));
    let right = condition.compare(Operation::Lt, input(3), input(5));
    let more = condition.logic(Operation::Or, left, right);
    graphs.push(condition.finish(vec![more]));
    let mut body = G::new(&format!("{prefix}-body"), state.clone(), state.clone());
    let left_available = body.compare(Operation::Lt, input(2), input(4));
    let right_done = body.compare(Operation::Ge, input(3), input(5));
    let left_key = call(
        &mut body,
        "control-fast-edge-key",
        vec![input(0), input(2)],
        int(),
    );
    let right_key = call(
        &mut body,
        "control-fast-edge-key",
        vec![input(0), input(3)],
        int(),
    );
    let before = body.compare(Operation::Le, left_key, right_key);
    let take_left = body.and(left_available, before);
    let take_left = body.logic(Operation::Or, right_done, take_left);
    let index = body.pick("reader-pick-offset", take_left.clone(), input(3), input(2));
    let row = body.index(input(0), index);
    let output = append(&mut body, input(1), row, rows());
    let left_next = body.advance(input(2), 1);
    let right_next = body.advance(input(3), 1);
    let left = body.pick("reader-pick-offset", take_left.clone(), input(2), left_next);
    let right = body.pick("reader-pick-offset", take_left, right_next, input(3));
    graphs.push(body.finish(vec![input(0), output, left, right, input(4), input(5)]));
    let mut merge = G::new(prefix, state.clone(), vec![rows()]);
    let out = merge.loop_node(prefix, state, (0..6).map(input).collect());
    graphs.push(merge.finish(vec![out[1].clone()]));

    let state = vec![rows(), int(), int(), rows()];
    let prefix = "control-fast-edges-pass";
    let mut condition = G::new(
        &format!("{prefix}-condition"),
        state.clone(),
        vec![SemanticType::Bool],
    );
    let count = condition.length(input(0));
    let more = condition.compare(Operation::Lt, input(2), count);
    graphs.push(condition.finish(vec![more]));
    let mut body = G::new(&format!("{prefix}-body"), state.clone(), state.clone());
    let count = len(&mut body, input(0));
    let middle = body.arithmetic(Operation::Add, input(2), input(1));
    let inside = body.compare(Operation::Le, middle.clone(), count.clone());
    let middle = body.pick("reader-pick-offset", inside, count.clone(), middle);
    let end = body.arithmetic(Operation::Add, middle.clone(), input(1));
    let inside = body.compare(Operation::Le, end.clone(), count.clone());
    let end = body.pick("reader-pick-offset", inside, count, end);
    let output = call(
        &mut body,
        "control-fast-edges-merge",
        vec![
            input(0),
            input(3),
            input(2),
            middle.clone(),
            middle,
            end.clone(),
        ],
        rows(),
    );
    graphs.push(body.finish(vec![input(0), input(1), end, output]));
    let mut pass = G::new(prefix, vec![rows(), int()], vec![rows()]);
    let zero = pass.n(0);
    let empty = array(&mut pass, vec![], rows());
    let out = pass.loop_node(prefix, state, vec![input(0), input(1), zero, empty]);
    graphs.push(pass.finish(vec![out[3].clone()]));

    let prefix = "control-fast-edges-merge-sort";
    let state = vec![rows(), int()];
    let mut condition = G::new(
        &format!("{prefix}-condition"),
        state.clone(),
        vec![SemanticType::Bool],
    );
    let count = condition.length(input(0));
    let more = condition.compare(Operation::Lt, input(1), count);
    graphs.push(condition.finish(vec![more]));
    let mut body = G::new(&format!("{prefix}-body"), state.clone(), state.clone());
    let output = call(
        &mut body,
        "control-fast-edges-pass",
        vec![input(0), input(1)],
        rows(),
    );
    let two = body.n(2);
    let width = body.arithmetic(Operation::Mul, input(1), two);
    graphs.push(body.finish(vec![output, width]));
    let mut main = G::new(prefix, vec![rows()], vec![rows()]);
    let one = main.n(1);
    let out = main.loop_node(prefix, state, vec![input(0), one]);
    graphs.push(main.finish(vec![out[0].clone()]));
    graphs
}

fn cache_graphs() -> Vec<Graph> {
    let mut graphs = vec![];
    // The source tables are trusted products of validated GIR lowering. The
    // headers occupy all scanner fields and use impossible endpoint/node tags.
    let edge_args = vec![rows(), int()];
    let mut node = G::new("control-fast-edge-node-key", edge_args.clone(), vec![int()]);
    let row = node.index(input(0), input(1));
    let node_key = node.field(row, 4);
    graphs.push(node.finish(vec![node_key]));
    let mut output = G::new(
        "control-fast-edge-output-key",
        edge_args.clone(),
        vec![int()],
    );
    let sentinel = output.n(1_i128 << 32);
    graphs.push(output.finish(vec![sentinel]));
    let mut key = G::new("control-fast-edge-key", edge_args, vec![int()]);
    let row = key.index(input(0), input(1));
    let kind = key.field(row, 3);
    let node = eq(&mut key, kind, 0);
    let value = choose(
        &mut key,
        node,
        "control-fast-edge-node-key",
        "control-fast-edge-output-key",
        vec![input(0), input(1)],
        int(),
    );
    graphs.push(key.finish(vec![value]));
    for slots in [true, false] {
        let prefix = if slots {
            "control-fast-slots-sorted"
        } else {
            "control-fast-edges-sorted"
        };
        let state = if slots {
            vec![
                rows(),
                int(),
                int(),
                int(),
                SemanticType::Bool,
                SemanticType::Bool,
            ]
        } else {
            vec![rows(), int(), int(), SemanticType::Bool, SemanticType::Bool]
        };
        let valid_at = if slots { 5 } else { 4 };
        let first_at = if slots { 4 } else { 3 };
        let mut c = G::new(
            &format!("{prefix}-condition"),
            state.clone(),
            vec![SemanticType::Bool],
        );
        let length = c.length(input(0));
        let more = c.compare(Operation::Lt, input(1), length);
        let more = c.and(more, input(valid_at));
        graphs.push(c.finish(vec![more]));
        let mut b = G::new(&format!("{prefix}-body"), state.clone(), state.clone());
        let mut next_state = vec![input(0), { b.advance(input(1), 1) }];
        let ordered = if slots {
            let row = b.index(input(0), input(1));
            let node = b.field(row.clone(), 0);
            let port = b.field(row, 1);
            let after = b.compare(Operation::Gt, node.clone(), input(2));
            let same = b.compare(Operation::Eq, node.clone(), input(2));
            let port_after = b.compare(Operation::Ge, port.clone(), input(3));
            let same = b.and(same, port_after);
            let ordered = b.logic(Operation::Or, after, same);
            next_state.extend([node, port]);
            ordered
        } else {
            let key = call(
                &mut b,
                "control-fast-edge-key",
                vec![input(0), input(1)],
                int(),
            );
            let ordered = b.compare(Operation::Ge, key.clone(), input(2));
            next_state.push(key);
            ordered
        };
        let ordered = b.logic(Operation::Or, input(first_at), ordered);
        let valid = b.and(input(valid_at), ordered);
        let no = b.bool(false);
        next_state.extend([no, valid]);
        graphs.push(b.finish(next_state));
        let mut main = G::new(prefix, vec![rows()], vec![SemanticType::Bool]);
        let zero = main.n(0);
        let yes = main.bool(true);
        let mut values = vec![input(0), zero.clone(), zero.clone()];
        if slots {
            values.push(zero);
        }
        values.extend([yes.clone(), yes]);
        let out = main.loop_node(prefix, state, values);
        graphs.push(main.finish(vec![out[valid_at as usize].clone()]));
        let mut cache = G::new(
            if slots {
                "control-fast-slots-cache"
            } else {
                "control-fast-edges-cache"
            },
            vec![rows()],
            vec![rows()],
        );
        let mut source = input(0);
        if !slots {
            let sorted = call(&mut cache, prefix, vec![source.clone()], SemanticType::Bool);
            source = choose(
                &mut cache,
                sorted,
                "control-fast-edges-identity",
                "control-fast-edges-merge-sort",
                vec![source],
                rows(),
            );
        }
        let sorted = call(&mut cache, prefix, vec![source.clone()], SemanticType::Bool);
        let flag = choose(
            &mut cache,
            sorted,
            "validator-name-one",
            "validator-name-zero",
            vec![],
            int(),
        );
        let count = len(&mut cache, input(0));
        let header = if slots {
            let sentinel = cache.n(1_i128 << 32);
            array(&mut cache, vec![sentinel, flag, count], seq())
        } else {
            let invalid = cache.n(2);
            let sentinel = cache.n(1_i128 << 32);
            array(
                &mut cache,
                vec![
                    invalid.clone(),
                    flag,
                    count,
                    invalid,
                    sentinel.clone(),
                    sentinel,
                ],
                seq(),
            )
        };
        let first = array(&mut cache, vec![header], rows());
        let result = cache.op(
            Operation::ArrayConcat,
            vec![(first, rows()), (source, rows())],
            rows(),
        );
        graphs.push(cache.finish(vec![result]));
    }
    graphs
}

fn binary_graphs() -> Vec<Graph> {
    let mut graphs = vec![];
    let compare_args = vec![rows(), int(), int(), int()];
    let mut slot = G::new(
        "control-fast-slot-less",
        compare_args.clone(),
        vec![SemanticType::Bool],
    );
    let row = slot.index(input(0), input(1));
    let node = slot.field(row.clone(), 0);
    let port = slot.field(row, 1);
    let before = slot.compare(Operation::Lt, node.clone(), input(2));
    let same = slot.compare(Operation::Eq, node, input(2));
    let port_before = slot.compare(Operation::Lt, port, input(3));
    let same = slot.and(same, port_before);
    let less = slot.logic(Operation::Or, before, same);
    graphs.push(slot.finish(vec![less]));
    let mut edge = G::new(
        "control-fast-edge-less",
        compare_args,
        vec![SemanticType::Bool],
    );
    let key = call(
        &mut edge,
        "control-fast-edge-key",
        vec![input(0), input(1)],
        int(),
    );
    let less = edge.compare(Operation::Lt, key, input(2));
    graphs.push(edge.finish(vec![less]));
    for (kind, compare) in [
        ("slot", "control-fast-slot-less"),
        ("edge", "control-fast-edge-less"),
    ] {
        let prefix = format!("control-fast-{kind}-bound");
        let state = vec![rows(), int(), int(), int(), int()];
        let mut c = G::new(
            &format!("{prefix}-condition"),
            state.clone(),
            vec![SemanticType::Bool],
        );
        let more = c.compare(Operation::Lt, input(3), input(4));
        graphs.push(c.finish(vec![more]));
        let mut b = G::new(&format!("{prefix}-body"), state.clone(), state.clone());
        let sum = b.arithmetic(Operation::Add, input(3), input(4));
        let two = b.n(2);
        let sum_ty = b.ty(&sum);
        let two_ty = b.ty(&two);
        let mid = b.op(Operation::Div, vec![(sum, sum_ty), (two, two_ty)], int());
        let less = call(
            &mut b,
            compare,
            vec![input(0), mid.clone(), input(1), input(2)],
            SemanticType::Bool,
        );
        let next = b.advance(mid.clone(), 1);
        let lo = b.pick("reader-pick-offset", less.clone(), input(3), next);
        let hi = b.pick("reader-pick-offset", less, mid, input(4));
        graphs.push(b.finish(vec![input(0), input(1), input(2), lo, hi]));
        let mut main = G::new(&prefix, vec![rows(), int(), int()], vec![int()]);
        let one = main.n(1);
        let count = len(&mut main, input(0));
        let out = main.loop_node(
            &prefix,
            state,
            vec![input(0), input(1), input(2), one, count],
        );
        graphs.push(main.finish(vec![out[3].clone()]));
    }
    let base = vec![rows(), int(), int()];
    let mut extended = base.clone();
    extended.push(int());
    let mut missing = G::new("control-fast-slot-missing", extended.clone(), vec![int()]);
    let zero = missing.n(0);
    graphs.push(missing.finish(vec![zero]));
    let mut found = G::new("control-fast-slot-found", extended.clone(), vec![int()]);
    let row = found.index(input(0), input(3));
    let slot = found.field(row, 2);
    graphs.push(found.finish(vec![slot]));
    let mut known = G::new("control-fast-slot-known", base.clone(), vec![int()]);
    let index = call(
        &mut known,
        "control-fast-slot-bound",
        vec![input(0), input(1), input(2)],
        int(),
    );
    let length = known.length(input(0));
    let inside = known.compare(Operation::Lt, index.clone(), length);
    let row = known.index(input(0), index.clone());
    let node = known.field(row.clone(), 0);
    let port = known.field(row, 1);
    let node_ok = known.compare(Operation::Eq, node, input(1));
    let port_ok = known.compare(Operation::Eq, port, input(2));
    let equal = known.and(node_ok, port_ok);
    let valid = known.and(inside, equal);
    let result = choose(
        &mut known,
        valid,
        "control-fast-slot-found",
        "control-fast-slot-missing",
        vec![input(0), input(1), input(2), index],
        int(),
    );
    graphs.push(known.finish(vec![result]));
    let mut main = G::new("control-fast-slot-index", base, vec![int()]);
    let zero = main.n(0);
    let header = main.index(input(0), zero);
    let tag = main.field(header.clone(), 0);
    let flag = main.field(header, 1);
    let magic = eq(&mut main, tag, 1_i128 << 32);
    let ordered = eq(&mut main, flag, 1);
    let valid = main.and(magic, ordered);
    let result = choose(
        &mut main,
        valid,
        "control-fast-slot-known",
        "control-slot-index",
        vec![input(0), input(1), input(2)],
        int(),
    );
    graphs.push(main.finish(vec![result]));
    graphs
}

fn argument_graphs() -> Vec<Graph> {
    let base = vec![SemanticType::Bytes, seq(), rows(), rows(), int()];
    let mut state = base.clone();
    state.extend([int(), int(), texts()]);
    let mut graphs = vec![];
    let mut c = G::new(
        "control-fast-argument-range-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let more = c.compare(Operation::Lt, input(5), input(6));
    graphs.push(c.finish(vec![more]));
    let mut b = G::new(
        "control-fast-argument-range-body",
        state.clone(),
        state.clone(),
    );
    let edge = b.index(input(3), input(5));
    let code = call(
        &mut b,
        "control-argument-edge",
        vec![input(0), edge, input(2), input(4), input(1)],
        SemanticType::Text,
    );
    let chunks = append(&mut b, input(7), code, texts());
    let next = b.advance(input(5), 1);
    graphs.push(b.finish(vec![
        input(0),
        input(1),
        input(2),
        input(3),
        input(4),
        next,
        input(6),
        chunks,
    ]));
    let mut known = G::new(
        "control-fast-arguments-known",
        base.clone(),
        vec![SemanticType::Text],
    );
    let id = known.field(input(1), 0);
    let zero = known.n(0);
    let start = call(
        &mut known,
        "control-fast-edge-bound",
        vec![input(3), id.clone(), zero.clone()],
        int(),
    );
    let next = known.advance(id, 1);
    let stop = call(
        &mut known,
        "control-fast-edge-bound",
        vec![input(3), next, zero],
        int(),
    );
    let e = array(&mut known, vec![], texts());
    let out = known.loop_node(
        "control-fast-argument-range",
        state,
        vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            start,
            stop,
            e,
        ],
    );
    let separator = known.text("");
    let result = known.op(
        Operation::TextJoin,
        vec![(out[7].clone(), texts()), (separator, SemanticType::Text)],
        SemanticType::Text,
    );
    graphs.push(known.finish(vec![result]));
    let mut main = G::new("control-fast-arguments", base, vec![SemanticType::Text]);
    let zero = main.n(0);
    let header = main.index(input(3), zero);
    let source = main.field(header.clone(), 0);
    let flag = main.field(header.clone(), 1);
    let target = main.field(header, 3);
    let magic = eq(&mut main, source, 2);
    let target = eq(&mut main, target, 2);
    let ordered = eq(&mut main, flag, 1);
    let valid = main.and(magic, target);
    let valid = main.and(valid, ordered);
    let result = choose(
        &mut main,
        valid,
        "control-fast-arguments-known",
        "control-arguments",
        (0..5).map(input).collect(),
        SemanticType::Text,
    );
    graphs.push(main.finish(vec![result]));
    graphs
}

pub(super) fn graphs() -> Vec<Graph> {
    let mut graphs = cache_graphs();
    graphs.extend(edge_sort_graphs());
    graphs.extend(binary_graphs());
    graphs.extend(argument_graphs());
    graphs
}
