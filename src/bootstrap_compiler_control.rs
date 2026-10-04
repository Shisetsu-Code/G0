//! Multi-graph native emission algorithms expressed as GIR. Rust constructs
//! these definitions; source documents are transformed only by these graphs.
use super::*;
fn seq() -> SemanticType {
    SemanticType::Slice(Box::new(int()))
}
fn rows() -> SemanticType {
    SemanticType::Slice(Box::new(seq()))
}
fn texts() -> SemanticType {
    SemanticType::Slice(Box::new(SemanticType::Text))
}
fn call(g: &mut G, name: &str, args: Vec<SourceEndpoint>, output: SemanticType) -> SourceEndpoint {
    let args = args
        .into_iter()
        .map(|e| {
            let mut t = g.ty(&e);
            if matches!(&t, SemanticType::Integer(range) if range.min >= 0 && range.max <= (1_i128 << 48)) {
                t = int();
            }
            (e, t)
        })
        .collect();
    g.op(Operation::Subgraph(name.into()), args, output)
}
fn multi(
    g: &mut G,
    name: &str,
    args: Vec<SourceEndpoint>,
    outputs: Vec<SemanticType>,
) -> Vec<SourceEndpoint> {
    let id = g.b.graph.nodes.len() as u32 + 1;
    let first = outputs.first().cloned().unwrap_or_else(int);
    call(g, name, args, first);
    g.b.graph.nodes.last_mut().unwrap().outputs = outputs
        .into_iter()
        .enumerate()
        .map(|(i, t)| port(i as u16, t))
        .collect();
    (0..g.b.graph.nodes.last().unwrap().outputs.len())
        .map(|p| SourceEndpoint::NodeOutput {
            node: id,
            port: p as u16,
        })
        .collect()
}
fn array(g: &mut G, values: Vec<SourceEndpoint>, ty: SemanticType) -> SourceEndpoint {
    let args = values
        .into_iter()
        .map(|e| {
            let t = g.ty(&e);
            (e, t)
        })
        .collect();
    g.op(Operation::MakeArray, args, ty)
}
fn append(
    g: &mut G,
    current: SourceEndpoint,
    value: SourceEndpoint,
    ty: SemanticType,
) -> SourceEndpoint {
    let singleton = array(g, vec![value], ty.clone());
    g.op(
        Operation::ArrayConcat,
        vec![(current, ty.clone()), (singleton, ty.clone())],
        ty,
    )
}
fn mod_id(g: &mut G, at: SourceEndpoint) -> SourceEndpoint {
    let word = g.u32(at);
    let modulus = g.n(65536);
    g.op(Operation::Rem, vec![(word, int()), (modulus, int())], int())
}

enum Fragment<'a> {
    Literal(&'a str),
    Number(SourceEndpoint),
    Text(SourceEndpoint),
}
fn template(g: &mut G, fragments: Vec<Fragment<'_>>) -> SourceEndpoint {
    let parts = fragments
        .into_iter()
        .map(|f| match f {
            Fragment::Literal(s) => g.text(s),
            Fragment::Number(e) => g.number(e),
            Fragment::Text(e) => e,
        })
        .collect();
    let parts = array(g, parts, texts());
    let separator = g.text("");
    g.op(
        Operation::TextJoin,
        vec![(parts, texts()), (separator, SemanticType::Text)],
        SemanticType::Text,
    )
}
fn choose(
    g: &mut G,
    test: SourceEndpoint,
    yes: &str,
    no: &str,
    args: Vec<SourceEndpoint>,
    output: SemanticType,
) -> SourceEndpoint {
    let mut ports = vec![(test, SemanticType::Bool)];
    ports.extend(args.into_iter().map(|e| {
        let mut t = g.ty(&e);
        if matches!(&t, SemanticType::Integer(range) if range.min >= 0 && range.max <= (1_i128 << 48)) {
            t = int();
        }
        (e, t)
    }));
    let result = g.op(
        Operation::Select {
            when_true: yes.into(),
            when_false: no.into(),
        },
        ports,
        output,
    );
    let node = g.b.graph.nodes.last_mut().unwrap();
    node.inputs[0].name = "selector".into();
    for (i, p) in node.inputs[1..].iter_mut().enumerate() {
        p.name = format!("p{i}");
    }
    result
}
fn join(g: &mut G, chunks: SourceEndpoint) -> SourceEndpoint {
    let empty = g.text("");
    g.op(
        Operation::TextJoin,
        vec![(chunks, texts()), (empty, SemanticType::Text)],
        SemanticType::Text,
    )
}

fn argument_graphs() -> Vec<Graph> {
    let source_types = vec![SemanticType::Bytes, seq(), rows(), int()];
    let mut from_input = G::new(
        "control-load-input",
        source_types.clone(),
        vec![SemanticType::Text],
    );
    let port = from_input.field(input(1), 2);
    let rank = call(
        &mut from_input,
        "control-port-rank",
        vec![input(0), input(3), port],
        int(),
    );
    let eight = from_input.n(8);
    let offset = from_input.arithmetic(Operation::Mul, rank, eight);
    let code_input = from_input.line("    movq ", offset, "(%r13), %rax\n");
    let mut from_node = G::new("control-load-node", source_types, vec![SemanticType::Text]);
    let node = from_node.field(input(1), 1);
    let port = from_node.field(input(1), 2);
    let slot = call(
        &mut from_node,
        "control-fast-slot-index",
        vec![input(2), node, port],
        int(),
    );
    let eight = from_node.n(8);
    let offset = from_node.arithmetic(Operation::Mul, slot, eight);
    let code_node = from_node.line("    movq ", offset, "(%r14), %rax\n");
    let emit_types = vec![SemanticType::Bytes, seq(), rows(), int(), seq()];
    let mut emit = G::new(
        "control-argument-edge",
        emit_types.clone(),
        vec![SemanticType::Text],
    );
    let kind = emit.field(input(1), 0);
    let zero = emit.n(0);
    let graph_input = emit.compare(Operation::Eq, kind, zero);
    let load = choose(
        &mut emit,
        graph_input,
        "control-load-input",
        "control-load-node",
        vec![input(0), input(1), input(2), input(3)],
        SemanticType::Text,
    );
    let ports = emit.field(input(4), 3);
    let target = emit.field(input(1), 5);
    let rank = call(
        &mut emit,
        "control-port-rank",
        vec![input(0), ports, target],
        int(),
    );
    let eight = emit.n(8);
    let offset = emit.arithmetic(Operation::Mul, rank, eight);
    let store = emit.line("    movq %rax, ", offset, "(%r15)\n");
    let edge_code = emit.concat(load, store);
    let mut skip = G::new(
        "control-argument-skip",
        emit_types,
        vec![SemanticType::Text],
    );
    let skip_code = skip.text("");
    let state = vec![
        SemanticType::Bytes,
        seq(),
        rows(),
        rows(),
        int(),
        int(),
        texts(),
    ];
    let mut cond = G::new(
        "control-arguments-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(3));
    let test = cond.compare(Operation::Lt, input(5), len);
    let mut body = G::new("control-arguments-body", state.clone(), state.clone());
    let edge = body.index(input(3), input(5));
    let target_kind = body.field(edge.clone(), 3);
    let zero = body.n(0);
    let node_input = body.compare(Operation::Eq, target_kind, zero);
    let target = body.field(edge.clone(), 4);
    let id = body.field(input(1), 0);
    let same = body.compare(Operation::Eq, target, id);
    let needed = body.and(node_input, same);
    let code = choose(
        &mut body,
        needed,
        "control-argument-edge",
        "control-argument-skip",
        vec![input(0), edge, input(2), input(4), input(1)],
        SemanticType::Text,
    );
    let parts = append(&mut body, input(6), code, texts());
    let next = body.advance(input(5), 1);
    let mut main = G::new(
        "control-arguments",
        vec![SemanticType::Bytes, seq(), rows(), rows(), int()],
        vec![SemanticType::Text],
    );
    let zero = main.n(0);
    let empty = array(&mut main, vec![], texts());
    let out = main.loop_node(
        "control-arguments",
        state,
        vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            zero,
            empty,
        ],
    );
    let result = join(&mut main, out[6].clone());
    vec![
        from_input.finish(vec![code_input]),
        from_node.finish(vec![code_node]),
        emit.finish(vec![edge_code]),
        skip.finish(vec![skip_code]),
        cond.finish(vec![test]),
        body.finish(vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            next,
            parts,
        ]),
        main.finish(vec![result]),
    ]
}

fn operation_types() -> Vec<SemanticType> {
    vec![SemanticType::Bytes, int(), int(), seq()]
}
fn label(g: &mut G, suffix: &str) -> SourceEndpoint {
    template(
        g,
        vec![
            Fragment::Literal(".Lg0_control_"),
            Fragment::Number(input(1)),
            Fragment::Literal("_"),
            Fragment::Number(input(2)),
            Fragment::Literal(suffix),
        ],
    )
}
fn failure(g: &mut G) -> SourceEndpoint {
    template(
        g,
        vec![
            Fragment::Literal(".Lg0_control_"),
            Fragment::Number(input(1)),
            Fragment::Literal("_fail"),
        ],
    )
}
fn node_count(g: &mut G) -> SourceEndpoint {
    let ports = g.field(input(3), 3);
    g.u32(ports)
}
fn control_start(g: &mut G) -> SourceEndpoint {
    let count = node_count(g);
    let fail = failure(g);
    template(
        g,
        vec![
            Fragment::Literal("    movq %r12, %rdi\n    movq $"),
            Fragment::Number(input(1)),
            Fragment::Literal(", %rsi\n    movq $"),
            Fragment::Number(input(2)),
            Fragment::Literal(", %rdx\n    movq %r15, %rcx\n    movq $"),
            Fragment::Number(count),
            Fragment::Literal(
                ", %r8\n    call g0_native_control_begin\n    testq %rax, %rax\n    jz ",
            ),
            Fragment::Text(fail),
            Fragment::Literal("\n"),
        ],
    )
}
fn native_call(
    g: &mut G,
    symbol: SourceEndpoint,
    count: SourceEndpoint,
    offset: u64,
) -> SourceEndpoint {
    template(
        g,
        vec![
            Fragment::Literal("    movq %r12, %rdi\n    leaq "),
            Fragment::Literal(if offset == 0 { "0" } else { "8" }),
            Fragment::Literal("(%r15), %rsi\n    movq $"),
            Fragment::Number(count),
            Fragment::Literal(", %rdx\n    call "),
            Fragment::Text(symbol),
            Fragment::Literal("\n"),
        ],
    )
}
fn operation_graphs() -> Vec<Graph> {
    let mut primitive = G::new(
        "control-operation-primitive",
        operation_types(),
        vec![SemanticType::Text],
    );
    let count = node_count(&mut primitive);
    let prim = template(
        &mut primitive,
        vec![
            Fragment::Literal("    movq %r12, %rdi\n    movq $"),
            Fragment::Number(input(1)),
            Fragment::Literal(", %rsi\n    movq $"),
            Fragment::Number(input(2)),
            Fragment::Literal(", %rdx\n    movq %r15, %rcx\n    movq $"),
            Fragment::Number(count),
            Fragment::Literal(", %r8\n    call g0_native_primitive\n"),
        ],
    );
    let mut sub = G::new(
        "control-operation-subgraph",
        operation_types(),
        vec![SemanticType::Text],
    );
    let start = control_start(&mut sub);
    let operation = sub.field(input(3), 2);
    let target = sub.advance(operation, 1);
    let symbol = call(
        &mut sub,
        "control-symbol",
        vec![input(0), target],
        SemanticType::Text,
    );
    let count = node_count(&mut sub);
    let invoke = native_call(&mut sub, symbol, count, 0);
    let sub_code = sub.concat(start, invoke);
    let mut select = G::new(
        "control-operation-select",
        operation_types(),
        vec![SemanticType::Text],
    );
    let start = control_start(&mut select);
    let operation = select.field(input(3), 2);
    let yes = select.advance(operation, 1);
    let no = select.call("reader-string", yes.clone());
    let yes = call(
        &mut select,
        "control-symbol",
        vec![input(0), yes],
        SemanticType::Text,
    );
    let no = call(
        &mut select,
        "control-symbol",
        vec![input(0), no],
        SemanticType::Text,
    );
    let count = node_count(&mut select);
    let one = select.n(1);
    let count = select.arithmetic(Operation::Sub, count, one);
    let yes_call = native_call(&mut select, yes, count.clone(), 8);
    let no_call = native_call(&mut select, no, count, 8);
    let otherwise = label(&mut select, "_else");
    let done = label(&mut select, "_done");
    let select_code = template(
        &mut select,
        vec![
            Fragment::Text(start),
            Fragment::Literal(
                "    movq %r12, %rdi\n    movq 0(%r15), %rsi\n    call g0_native_truth\n    testq %rax, %rax\n    jz ",
            ),
            Fragment::Text(otherwise.clone()),
            Fragment::Literal("\n"),
            Fragment::Text(yes_call),
            Fragment::Literal("    jmp "),
            Fragment::Text(done.clone()),
            Fragment::Literal("\n"),
            Fragment::Text(otherwise),
            Fragment::Literal(":\n"),
            Fragment::Text(no_call),
            Fragment::Text(done),
            Fragment::Literal(":\n"),
        ],
    );
    let mut main = G::new(
        "control-operation",
        operation_types(),
        vec![SemanticType::Text],
    );
    let op = main.field(input(3), 2);
    let tag = main.byte(op);
    let four = main.n(4);
    let select_test = main.compare(Operation::Eq, tag, four);
    let selected = choose(
        &mut main,
        select_test,
        "control-operation-select",
        "control-operation-other",
        vec![input(0), input(1), input(2), input(3)],
        SemanticType::Text,
    );
    let mut selector = G::new(
        "control-operation-other",
        operation_types(),
        vec![SemanticType::Text],
    );
    let op = selector.field(input(3), 2);
    let tag = selector.byte(op);
    let seven = selector.n(7);
    let sub_test = selector.compare(Operation::Eq, tag, seven);
    let chosen = choose(
        &mut selector,
        sub_test,
        "control-operation-subgraph",
        "control-operation-loop-or",
        vec![input(0), input(1), input(2), input(3)],
        SemanticType::Text,
    );
    vec![
        primitive.finish(vec![prim]),
        sub.finish(vec![sub_code]),
        select.finish(vec![select_code]),
        selector.finish(vec![chosen]),
        main.finish(vec![selected]),
    ]
}
fn loop_graphs() -> Vec<Graph> {
    let small = SemanticType::Integer(IntegerType {
        min: 0,
        max: u32::MAX as i128,
    });
    let wide = SemanticType::Integer(IntegerType {
        min: 0,
        max: u64::MAX as i128,
    });
    let scaled = SemanticType::Integer(IntegerType {
        min: 0,
        max: (u32::MAX as i128) * 4294967296,
    });
    let mut word = G::new(
        "control-u64-text",
        vec![SemanticType::Bytes, int()],
        vec![SemanticType::Text],
    );
    let low = word.u32(input(1));
    let next = word.advance(input(1), 4);
    let high = word.u32(next);
    let low = word.op(Operation::ConvertChecked, vec![(low, int())], small.clone());
    let high = word.op(
        Operation::ConvertChecked,
        vec![(high, int())],
        small.clone(),
    );
    let base_type = SemanticType::Integer(IntegerType {
        min: 4294967296,
        max: 4294967296,
    });
    let base = word.op(
        Operation::Const(Literal::Integer(4294967296)),
        vec![],
        base_type.clone(),
    );
    let high = word.op(
        Operation::Mul,
        vec![(high, small.clone()), (base, base_type)],
        scaled.clone(),
    );
    let value = word.op(Operation::Add, vec![(high, scaled), (low, small)], wide);
    let number = word.number(value);
    let state = vec![int(), int(), texts()];
    let mut cond = G::new(
        "control-loop-copy-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let test = cond.compare(Operation::Lt, input(1), input(0));
    let mut body = G::new("control-loop-copy-body", state.clone(), state.clone());
    let eight = body.n(8);
    let offset = body.arithmetic(Operation::Mul, input(1), eight);
    let code = template(
        &mut body,
        vec![
            Fragment::Literal("    movq %r12, %rdi\n    movq -72(%rbp), %rsi\n    movq $"),
            Fragment::Number(input(1)),
            Fragment::Literal(", %rdx\n    call g0_native_pack_item\n    movq %rax, "),
            Fragment::Number(offset),
            Fragment::Literal("(%r15)\n"),
        ],
    );
    let body_parts = append(&mut body, input(2), code, texts());
    let index = body.advance(input(1), 1);
    let mut copy_pack = G::new(
        "control-loop-copy-pack",
        vec![int()],
        vec![SemanticType::Text],
    );
    let zero = copy_pack.n(0);
    let empty = array(&mut copy_pack, vec![], texts());
    let out = copy_pack.loop_node("control-loop-copy", state, vec![input(0), zero, empty]);
    let parts = join(&mut copy_pack, out[2].clone());
    let copy_code = template(
        &mut copy_pack,
        vec![
            Fragment::Literal("    movq %rax, -72(%rbp)\n"),
            Fragment::Text(parts),
        ],
    );
    let mut single = G::new(
        "control-loop-copy-single",
        vec![int()],
        vec![SemanticType::Text],
    );
    let single_code = single.text("    movq %rax, 0(%r15)\n");
    let mut copy = G::new("control-loop-copy", vec![int()], vec![SemanticType::Text]);
    let one = copy.n(1);
    let is_single = copy.compare(Operation::Eq, input(0), one);
    let copied = choose(
        &mut copy,
        is_single,
        "control-loop-copy-single",
        "control-loop-copy-pack",
        vec![input(0)],
        SemanticType::Text,
    );
    let mut one_result = G::new(
        "control-loop-result-single",
        vec![int()],
        vec![SemanticType::Text],
    );
    let single_result = one_result.text("    movq 0(%r15), %rax\n");
    let mut pack_result = G::new(
        "control-loop-result-pack",
        vec![int()],
        vec![SemanticType::Text],
    );
    let pack_result_code = template(
        &mut pack_result,
        vec![
            Fragment::Literal("    movq %r12, %rdi\n    movq %r15, %rsi\n    movq $"),
            Fragment::Number(input(0)),
            Fragment::Literal(", %rdx\n    call g0_native_pack\n"),
        ],
    );
    let mut result = G::new("control-loop-result", vec![int()], vec![SemanticType::Text]);
    let one = result.n(1);
    let test_single = result.compare(Operation::Eq, input(0), one);
    let result_code = choose(
        &mut result,
        test_single,
        "control-loop-result-single",
        "control-loop-result-pack",
        vec![input(0)],
        SemanticType::Text,
    );
    let mut main = G::new(
        "control-operation-loop",
        operation_types(),
        vec![SemanticType::Text],
    );
    let start = control_start(&mut main);
    let operation = main.field(input(3), 2);
    let condition = main.advance(operation, 1);
    let body_offset = main.call("reader-string", condition.clone());
    let bound_offset = main.call("reader-string", body_offset.clone());
    let condition = call(
        &mut main,
        "control-symbol",
        vec![input(0), condition],
        SemanticType::Text,
    );
    let target = call(
        &mut main,
        "control-symbol",
        vec![input(0), body_offset],
        SemanticType::Text,
    );
    let bound = call(
        &mut main,
        "control-u64-text",
        vec![input(0), bound_offset],
        SemanticType::Text,
    );
    let count = node_count(&mut main);
    let condition_call = native_call(&mut main, condition, count.clone(), 0);
    let body_call = native_call(&mut main, target, count.clone(), 0);
    let copies = call(
        &mut main,
        "control-loop-copy",
        vec![count.clone()],
        SemanticType::Text,
    );
    let returned = call(
        &mut main,
        "control-loop-result",
        vec![count],
        SemanticType::Text,
    );
    let head = label(&mut main, "_head");
    let done = label(&mut main, "_done");
    let exceeded = label(&mut main, "_bound");
    let fail = failure(&mut main);
    let loop_code = template(
        &mut main,
        vec![
            Fragment::Text(start),
            Fragment::Literal("    movq $0, -48(%rbp)\n"),
            Fragment::Text(head.clone()),
            Fragment::Literal(
                ":\n    movq %r12, %rdi\n    call g0_native_tick\n    testq %rax, %rax\n    jz ",
            ),
            Fragment::Text(fail.clone()),
            Fragment::Literal("\n"),
            Fragment::Text(condition_call),
            Fragment::Literal("    testq %rax, %rax\n    jz "),
            Fragment::Text(fail.clone()),
            Fragment::Literal(
                "\n    movq %r12, %rdi\n    movq %rax, %rsi\n    call g0_native_truth\n    testq %rax, %rax\n    jz ",
            ),
            Fragment::Text(done.clone()),
            Fragment::Literal("\n    movabsq $"),
            Fragment::Text(bound),
            Fragment::Literal(", %rax\n    cmpq %rax, -48(%rbp)\n    jae "),
            Fragment::Text(exceeded.clone()),
            Fragment::Literal("\n"),
            Fragment::Text(body_call),
            Fragment::Literal("    testq %rax, %rax\n    jz "),
            Fragment::Text(fail.clone()),
            Fragment::Literal("\n"),
            Fragment::Text(copies),
            Fragment::Literal("    incq -48(%rbp)\n    jmp "),
            Fragment::Text(head),
            Fragment::Literal("\n"),
            Fragment::Text(exceeded),
            Fragment::Literal(":\n    movq %r12, %rdi\n    movq $"),
            Fragment::Number(input(1)),
            Fragment::Literal(", %rsi\n    movq $"),
            Fragment::Number(input(2)),
            Fragment::Literal(", %rdx\n    call g0_native_loop_fail\n    jmp "),
            Fragment::Text(fail),
            Fragment::Literal("\n"),
            Fragment::Text(done),
            Fragment::Literal(":\n"),
            Fragment::Text(returned),
        ],
    );
    let mut dispatch = G::new(
        "control-operation-loop-or",
        operation_types(),
        vec![SemanticType::Text],
    );
    let offset = dispatch.field(input(3), 2);
    let tag = dispatch.byte(offset);
    let six = dispatch.n(6);
    let test_loop = dispatch.compare(Operation::Eq, tag, six);
    let dispatch_code = choose(
        &mut dispatch,
        test_loop,
        "control-operation-loop",
        "control-operation-map-or",
        vec![input(0), input(1), input(2), input(3)],
        SemanticType::Text,
    );
    vec![
        word.finish(vec![number]),
        cond.finish(vec![test]),
        body.finish(vec![input(0), index, body_parts]),
        copy_pack.finish(vec![copy_code]),
        single.finish(vec![single_code]),
        copy.finish(vec![copied]),
        one_result.finish(vec![single_result]),
        pack_result.finish(vec![pack_result_code]),
        result.finish(vec![result_code]),
        main.finish(vec![loop_code]),
        dispatch.finish(vec![dispatch_code]),
    ]
}
fn check_result(g: &mut G) -> SourceEndpoint {
    let fail = failure(g);
    template(
        g,
        vec![
            Fragment::Literal("    testq %rax, %rax\n    jz "),
            Fragment::Text(fail),
            Fragment::Literal("\n"),
        ],
    )
}
fn map_graphs() -> Vec<Graph> {
    let mut main = G::new(
        "control-operation-map",
        operation_types(),
        vec![SemanticType::Text],
    );
    let start = control_start(&mut main);
    let op = main.field(input(3), 2);
    let name = main.advance(op, 1);
    let symbol = call(
        &mut main,
        "control-symbol",
        vec![input(0), name],
        SemanticType::Text,
    );
    let checked = check_result(&mut main);
    let head = label(&mut main, "_head");
    let done = label(&mut main, "_done");
    let code = template(
        &mut main,
        vec![
            Fragment::Text(start),
            Fragment::Literal(
                "    movq 0(%r15), %rsi\n    movq %rsi, -72(%rbp)\n    movq %r12, %rdi\n    call g0_native_sequence_len\n    movq %rax, -48(%rbp)\n    movq %r12, %rdi\n    movq %rax, %rsi\n    call g0_native_map_begin\n",
            ),
            Fragment::Text(checked.clone()),
            Fragment::Literal("    movq %rax, -56(%rbp)\n    movq $0, -64(%rbp)\n"),
            Fragment::Text(head.clone()),
            Fragment::Literal(":\n    movq -64(%rbp), %rax\n    cmpq -48(%rbp), %rax\n    jae "),
            Fragment::Text(done.clone()),
            Fragment::Literal(
                "\n    movq %r12, %rdi\n    movq -72(%rbp), %rsi\n    movq %rax, %rdx\n    call g0_native_sequence_item\n",
            ),
            Fragment::Text(checked.clone()),
            Fragment::Literal(
                "    movq %rax, -80(%rbp)\n    movq %r12, %rdi\n    leaq -80(%rbp), %rsi\n    movq $1, %rdx\n    call ",
            ),
            Fragment::Text(symbol),
            Fragment::Literal("\n"),
            Fragment::Text(checked.clone()),
            Fragment::Literal(
                "    movq %r12, %rdi\n    movq -56(%rbp), %rsi\n    movq %rax, %rdx\n    call g0_native_map_push\n",
            ),
            Fragment::Text(checked),
            Fragment::Literal("    incq -64(%rbp)\n    jmp "),
            Fragment::Text(head),
            Fragment::Literal("\n"),
            Fragment::Text(done),
            Fragment::Literal(":\n    movq %r12, %rdi\n    movq $"),
            Fragment::Number(input(1)),
            Fragment::Literal(", %rsi\n    movq $"),
            Fragment::Number(input(2)),
            Fragment::Literal(", %rdx\n    movq -56(%rbp), %rcx\n    call g0_native_map_finish\n"),
        ],
    );
    let mut dispatch = G::new(
        "control-operation-map-or",
        operation_types(),
        vec![SemanticType::Text],
    );
    let op = dispatch.field(input(3), 2);
    let tag = dispatch.byte(op);
    let expected = dispatch.n(47);
    let test = dispatch.compare(Operation::Eq, tag, expected);
    let dispatch_code = choose(
        &mut dispatch,
        test,
        "control-operation-map",
        "control-operation-match-or",
        vec![input(0), input(1), input(2), input(3)],
        SemanticType::Text,
    );
    vec![
        main.finish(vec![code]),
        dispatch.finish(vec![dispatch_code]),
    ]
}
fn match_graphs() -> Vec<Graph> {
    let mut state = operation_types();
    state.extend([int(), int(), int(), texts(), texts()]);
    let mut cond = G::new(
        "control-match-arms-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let test = cond.compare(Operation::Gt, input(5), zero);
    let mut body = G::new("control-match-arms-body", state.clone(), state.clone());
    let target = body.call("reader-string", input(4));
    let next = body.call("reader-string", target.clone());
    let symbol = call(
        &mut body,
        "control-symbol",
        vec![input(0), target],
        SemanticType::Text,
    );
    let count = node_count(&mut body);
    let one = body.n(1);
    let args = body.arithmetic(Operation::Sub, count, one.clone());
    let invoke = native_call(&mut body, symbol, args, 8);
    let prefix = label(&mut body, "_arm");
    let arm = template(
        &mut body,
        vec![Fragment::Text(prefix), Fragment::Number(input(6))],
    );
    let done = label(&mut body, "_done");
    let compare = template(
        &mut body,
        vec![
            Fragment::Literal("    cmpq $"),
            Fragment::Number(input(6)),
            Fragment::Literal(", %rax\n    je "),
            Fragment::Text(arm.clone()),
            Fragment::Literal("\n"),
        ],
    );
    let invocation = template(
        &mut body,
        vec![
            Fragment::Text(arm),
            Fragment::Literal(":\n"),
            Fragment::Text(invoke),
            Fragment::Literal("    jmp "),
            Fragment::Text(done),
            Fragment::Literal("\n"),
        ],
    );
    let comparisons = append(&mut body, input(7), compare, texts());
    let calls = append(&mut body, input(8), invocation, texts());
    let left = body.arithmetic(Operation::Sub, input(5), one);
    let index = body.advance(input(6), 1);
    let mut main = G::new(
        "control-operation-match",
        operation_types(),
        vec![SemanticType::Text],
    );
    let start = control_start(&mut main);
    let op = main.field(input(3), 2);
    let count_at = main.advance(op, 1);
    let count = main.u32(count_at.clone());
    let first = main.advance(count_at, 4);
    let zero = main.n(0);
    let empty = array(&mut main, vec![], texts());
    let out = main.loop_node(
        "control-match-arms",
        state,
        vec![
            input(0),
            input(1),
            input(2),
            input(3),
            first,
            count,
            zero,
            empty.clone(),
            empty,
        ],
    );
    let compares = join(&mut main, out[7].clone());
    let invocations = join(&mut main, out[8].clone());
    let default = call(
        &mut main,
        "control-symbol",
        vec![input(0), out[4].clone()],
        SemanticType::Text,
    );
    let count = node_count(&mut main);
    let one = main.n(1);
    let count = main.arithmetic(Operation::Sub, count, one);
    let default_call = native_call(&mut main, default, count, 8);
    let done = label(&mut main, "_done");
    let code = template(
        &mut main,
        vec![
            Fragment::Text(start),
            Fragment::Literal("    movq %r12, %rdi\n    movq $"),
            Fragment::Number(input(1)),
            Fragment::Literal(", %rsi\n    movq $"),
            Fragment::Number(input(2)),
            Fragment::Literal(", %rdx\n    movq 0(%r15), %rcx\n    call g0_native_match_arm\n"),
            Fragment::Text(compares),
            Fragment::Text(default_call),
            Fragment::Literal("    jmp "),
            Fragment::Text(done.clone()),
            Fragment::Literal("\n"),
            Fragment::Text(invocations),
            Fragment::Text(done),
            Fragment::Literal(":\n"),
        ],
    );
    let mut dispatch = G::new(
        "control-operation-match-or",
        operation_types(),
        vec![SemanticType::Text],
    );
    let op = dispatch.field(input(3), 2);
    let tag = dispatch.byte(op);
    let five = dispatch.n(5);
    let matched = dispatch.compare(Operation::Eq, tag, five);
    let dispatch_code = choose(
        &mut dispatch,
        matched,
        "control-operation-match",
        "control-operation-primitive",
        vec![input(0), input(1), input(2), input(3)],
        SemanticType::Text,
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![
            input(0),
            input(1),
            input(2),
            input(3),
            next,
            left,
            index,
            comparisons,
            calls,
        ]),
        main.finish(vec![code]),
        dispatch.finish(vec![dispatch_code]),
    ]
}
fn output_types() -> Vec<SemanticType> {
    vec![SemanticType::Bytes, int(), int(), seq(), rows()]
}
fn output_store(g: &mut G, port_id: SourceEndpoint) -> SourceEndpoint {
    let id = g.field(input(3), 0);
    let slot = call(
        g,
        "control-fast-slot-index",
        vec![input(4), id, port_id],
        int(),
    );
    let eight = g.n(8);
    let offset = g.arithmetic(Operation::Mul, slot, eight);
    g.line("    movq %rax, ", offset, "(%r14)\n")
}
fn output_graphs() -> Vec<Graph> {
    let mut single = G::new(
        "control-store-single",
        output_types(),
        vec![SemanticType::Text],
    );
    let list = single.field(input(3), 4);
    let at = single.advance(list, 4);
    let port = mod_id(&mut single, at);
    let single_code = output_store(&mut single, port);
    let mut types = output_types();
    types.extend([int(), int(), int(), texts()]);
    let mut cond = G::new(
        "control-store-pack-condition",
        types.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let test = cond.compare(Operation::Gt, input(6), zero);
    let mut body = G::new("control-store-pack-body", types.clone(), types.clone());
    let port = mod_id(&mut body, input(5));
    let store = output_store(&mut body, port);
    let code = template(
        &mut body,
        vec![
            Fragment::Literal("    movq %r12, %rdi\n    movq -40(%rbp), %rsi\n    movq $"),
            Fragment::Number(input(7)),
            Fragment::Literal(", %rdx\n    call g0_native_pack_item\n"),
            Fragment::Text(store),
        ],
    );
    let body_parts = append(&mut body, input(8), code, texts());
    let cursor = body.call("reader-port-layout", input(5));
    let one = body.n(1);
    let left = body.arithmetic(Operation::Sub, input(6), one);
    let ordinal = body.advance(input(7), 1);
    let mut pack = G::new(
        "control-store-pack",
        output_types(),
        vec![SemanticType::Text],
    );
    let ports = pack.field(input(3), 4);
    let count = pack.u32(ports.clone());
    let cursor_start = pack.advance(ports, 4);
    let zero = pack.n(0);
    let empty = array(&mut pack, vec![], texts());
    let out = pack.loop_node(
        "control-store-pack",
        types,
        vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            cursor_start,
            count,
            zero,
            empty,
        ],
    );
    let stores = join(&mut pack, out[8].clone());
    let fail = failure(&mut pack);
    let pack_code = template(
        &mut pack,
        vec![
            Fragment::Literal("    movq %rax, -40(%rbp)\n    movq %r12, %rdi\n    movq $"),
            Fragment::Number(input(1)),
            Fragment::Literal(", %rsi\n    movq $"),
            Fragment::Number(input(2)),
            Fragment::Literal(
                ", %rdx\n    movq %rax, %rcx\n    call g0_native_pack_check\n    testq %rax, %rax\n    jz ",
            ),
            Fragment::Text(fail),
            Fragment::Literal("\n"),
            Fragment::Text(stores),
        ],
    );
    let mut main = G::new(
        "control-store-outputs",
        output_types(),
        vec![SemanticType::Text],
    );
    let list = main.field(input(3), 4);
    let count = main.u32(list);
    let one = main.n(1);
    let is_single = main.compare(Operation::Eq, count, one);
    let main_code = choose(
        &mut main,
        is_single,
        "control-store-single",
        "control-store-pack",
        vec![input(0), input(1), input(2), input(3), input(4)],
        SemanticType::Text,
    );
    vec![
        single.finish(vec![single_code]),
        cond.finish(vec![test]),
        body.finish(vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            cursor,
            left,
            ordinal,
            body_parts,
        ]),
        pack.finish(vec![pack_code]),
        main.finish(vec![main_code]),
    ]
}
fn graph_output_graphs() -> Vec<Graph> {
    let edge_types = vec![SemanticType::Bytes, seq(), rows(), int(), int()];
    let mut emit = G::new(
        "control-graph-output-edge",
        edge_types.clone(),
        vec![SemanticType::Text],
    );
    let kind = emit.field(input(1), 0);
    let zero = emit.n(0);
    let is_input = emit.compare(Operation::Eq, kind, zero);
    let load = choose(
        &mut emit,
        is_input,
        "control-load-input",
        "control-load-node",
        vec![input(0), input(1), input(2), input(3)],
        SemanticType::Text,
    );
    let port = emit.field(input(1), 5);
    let rank = call(
        &mut emit,
        "control-port-rank",
        vec![input(0), input(4), port],
        int(),
    );
    let eight = emit.n(8);
    let offset = emit.arithmetic(Operation::Mul, rank, eight);
    let store = emit.line("    movq %rax, ", offset, "(%r15)\n");
    let edge_code = emit.concat(load, store);
    let mut skip = G::new(
        "control-graph-output-skip",
        edge_types,
        vec![SemanticType::Text],
    );
    let skip_code = skip.text("");
    let state = vec![
        SemanticType::Bytes,
        rows(),
        rows(),
        int(),
        int(),
        int(),
        texts(),
    ];
    let mut cond = G::new(
        "control-graph-outputs-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(2));
    let test = cond.compare(Operation::Lt, input(5), len);
    let mut body = G::new("control-graph-outputs-body", state.clone(), state.clone());
    let edge = body.index(input(2), input(5));
    let kind = body.field(edge.clone(), 3);
    let one = body.n(1);
    let needed = body.compare(Operation::Eq, kind, one);
    let code = choose(
        &mut body,
        needed,
        "control-graph-output-edge",
        "control-graph-output-skip",
        vec![input(0), edge, input(1), input(3), input(4)],
        SemanticType::Text,
    );
    let parts = append(&mut body, input(6), code, texts());
    let next = body.advance(input(5), 1);
    let mut main = G::new(
        "control-graph-outputs",
        vec![SemanticType::Bytes, rows(), rows(), int(), int()],
        vec![SemanticType::Text],
    );
    let zero = main.n(0);
    let empty = array(&mut main, vec![], texts());
    let out = main.loop_node(
        "control-graph-outputs",
        state,
        vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            zero,
            empty,
        ],
    );
    let main_code = join(&mut main, out[6].clone());
    vec![
        emit.finish(vec![edge_code]),
        skip.finish(vec![skip_code]),
        cond.finish(vec![test]),
        body.finish(vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            next,
            parts,
        ]),
        main.finish(vec![main_code]),
    ]
}
fn max_args_graphs() -> Vec<Graph> {
    let state = vec![SemanticType::Bytes, rows(), int(), int()];
    let mut cond = G::new(
        "control-max-args-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(1));
    let test = cond.compare(Operation::Lt, input(2), len);
    let mut body = G::new("control-max-args-body", state.clone(), state.clone());
    let descriptor = body.index(input(1), input(2));
    let ports = body.field(descriptor, 3);
    let count = body.u32(ports);
    let greater = body.compare(Operation::Gt, count.clone(), input(3));
    let maximum = body.pick("reader-pick-offset", greater, input(3), count);
    let next = body.advance(input(2), 1);
    let mut main = G::new(
        "control-max-args",
        vec![SemanticType::Bytes, rows(), int()],
        vec![int()],
    );
    let zero = main.n(0);
    let out = main.loop_node(
        "control-max-args",
        state,
        vec![input(0), input(1), zero, input(2)],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), next, maximum]),
        main.finish(vec![out[3].clone()]),
    ]
}
fn graph_node_graphs() -> Vec<Graph> {
    let state = vec![
        SemanticType::Bytes,
        int(),
        int(),
        rows(),
        rows(),
        rows(),
        rows(),
        int(),
        texts(),
    ];
    let mut cond = G::new(
        "control-graph-nodes-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(6));
    let test = cond.compare(Operation::Lt, input(7), len);
    let mut body = G::new("control-graph-nodes-body", state.clone(), state.clone());
    let descriptor = body.index(input(6), input(7));
    let id = body.field(descriptor.clone(), 0);
    let ordinal = call(
        &mut body,
        "validator-node-index-fast",
        vec![input(3), id],
        int(),
    );
    let args = call(
        &mut body,
        "control-fast-arguments",
        vec![input(0), descriptor.clone(), input(5), input(4), input(2)],
        SemanticType::Text,
    );
    let op = call(
        &mut body,
        "control-operation",
        vec![input(0), input(1), ordinal.clone(), descriptor.clone()],
        SemanticType::Text,
    );
    let store = call(
        &mut body,
        "control-store-outputs",
        vec![input(0), input(1), ordinal, descriptor, input(5)],
        SemanticType::Text,
    );
    let fail = failure(&mut body);
    let code = template(
        &mut body,
        vec![
            Fragment::Text(args),
            Fragment::Text(op),
            Fragment::Literal("    testq %rax, %rax\n    jz "),
            Fragment::Text(fail.clone()),
            Fragment::Literal("\n"),
            Fragment::Text(store),
            Fragment::Literal(
                "    movq %r12, %rdi\n    call g0_native_failed\n    testq %rax, %rax\n    jnz ",
            ),
            Fragment::Text(fail),
            Fragment::Literal("\n"),
        ],
    );
    let parts = append(&mut body, input(8), code, texts());
    let next = body.advance(input(7), 1);
    let mut main = G::new(
        "control-graph-nodes",
        state[..7].to_vec(),
        vec![SemanticType::Text],
    );
    let zero = main.n(0);
    let empty = array(&mut main, vec![], texts());
    let out = main.loop_node(
        "control-graph-nodes",
        state,
        vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            input(5),
            input(6),
            zero,
            empty,
        ],
    );
    let main_code = join(&mut main, out[8].clone());
    vec![
        cond.finish(vec![test]),
        body.finish(vec![
            input(0),
            input(1),
            input(2),
            input(3),
            input(4),
            input(5),
            input(6),
            next,
            parts,
        ]),
        main.finish(vec![main_code]),
    ]
}
fn graph_finish_graphs() -> Vec<Graph> {
    let types = vec![SemanticType::Bytes, int(), int()];
    let mut single = G::new(
        "control-graph-result-single",
        types.clone(),
        vec![SemanticType::Text],
    );
    let one_code = template(
        &mut single,
        vec![
            Fragment::Literal("    movq %rax, %rdx\n    movq %r12, %rdi\n    movq $"),
            Fragment::Number(input(1)),
            Fragment::Literal(", %rsi\n    call g0_native_graph_result\n"),
        ],
    );
    let mut pack = G::new(
        "control-graph-result-pack",
        types.clone(),
        vec![SemanticType::Text],
    );
    let pack_code = template(
        &mut pack,
        vec![
            Fragment::Literal("    movq %rax, %rcx\n    movq %r12, %rdi\n    movq $"),
            Fragment::Number(input(1)),
            Fragment::Literal(", %rsi\n    movq $-1, %rdx\n    call g0_native_pack_check\n"),
        ],
    );
    let mut main = G::new("control-graph-result", types, vec![SemanticType::Text]);
    let one = main.n(1);
    let test = main.compare(Operation::Eq, input(2), one);
    let code = choose(
        &mut main,
        test,
        "control-graph-result-single",
        "control-graph-result-pack",
        vec![input(0), input(1), input(2)],
        SemanticType::Text,
    );
    vec![
        single.finish(vec![one_code]),
        pack.finish(vec![pack_code]),
        main.finish(vec![code]),
    ]
}
fn graph_emission_graphs() -> Vec<Graph> {
    let mut main = G::new(
        "control-graph",
        vec![SemanticType::Bytes, int(), seq()],
        vec![SemanticType::Text],
    );
    let zero = main.n(0);
    let nodes = call(
        &mut main,
        "reader-graph-ast",
        vec![input(0), zero.clone()],
        rows(),
    );
    let edges = call(
        &mut main,
        "reader-graph-edges",
        vec![input(0), zero],
        rows(),
    );
    let ordered = call(
        &mut main,
        "scheduler-fast-rows",
        vec![nodes.clone(), edges.clone()],
        rows(),
    );
    let slots = multi(
        &mut main,
        "control-node-slots",
        vec![input(0), nodes.clone()],
        vec![int(), rows()],
    );
    let cached_slots = call(
        &mut main,
        "control-fast-slots-cache",
        vec![slots[1].clone()],
        rows(),
    );
    let cached_edges = call(
        &mut main,
        "control-fast-edges-cache",
        vec![edges.clone()],
        rows(),
    );
    let input_ports = main.field(input(2), 3);
    let output_ports = main.field(input(2), 4);
    let outputs = main.u32(output_ports.clone());
    let maximum = call(
        &mut main,
        "control-max-args",
        vec![input(0), nodes.clone(), outputs.clone()],
        int(),
    );
    let eight = main.n(8);
    let slot_bytes = main.arithmetic(Operation::Mul, slots[0].clone(), eight.clone());
    let total = main.arithmetic(Operation::Add, slots[0].clone(), maximum);
    let total = main.advance(total, 6);
    let bytes = main.arithmetic(Operation::Mul, total, eight);
    let rounded = main.advance(bytes, 15);
    let sixteen = main.n(16);
    let allocation = main.op(
        Operation::Div,
        vec![(rounded, int()), (sixteen.clone(), int())],
        int(),
    );
    let allocation = main.arithmetic(Operation::Mul, allocation, sixteen);
    let name_at = main.field(input(2), 2);
    let symbol = call(
        &mut main,
        "control-symbol",
        vec![input(0), name_at],
        SemanticType::Text,
    );
    let fail = failure(&mut main);
    let leave = template(
        &mut main,
        vec![
            Fragment::Literal(".Lg0_control_"),
            Fragment::Number(input(1)),
            Fragment::Literal("_leave"),
        ],
    );
    let early = template(
        &mut main,
        vec![
            Fragment::Literal(".Lg0_control_"),
            Fragment::Number(input(1)),
            Fragment::Literal("_early"),
        ],
    );
    let return_label = template(
        &mut main,
        vec![
            Fragment::Literal(".Lg0_control_"),
            Fragment::Number(input(1)),
            Fragment::Literal("_return"),
        ],
    );
    let prologue = template(
        &mut main,
        vec![
            Fragment::Literal(".text\n.type "),
            Fragment::Text(symbol.clone()),
            Fragment::Literal(", @function\n"),
            Fragment::Text(symbol.clone()),
            Fragment::Literal(
                ":\n    pushq %rbp\n    movq %rsp, %rbp\n    pushq %r12\n    pushq %r13\n    pushq %r14\n    pushq %r15\n    movq %rdi, %r12\n    movq %rsi, %r13\n    movq %rdx, %rcx\n    movq %r13, %rdx\n    movq $",
            ),
            Fragment::Number(input(1)),
            Fragment::Literal(
                ", %rsi\n    call g0_native_graph_enter\n    testq %rax, %rax\n    jz ",
            ),
            Fragment::Text(early.clone()),
            Fragment::Literal("\n    subq $"),
            Fragment::Number(allocation.clone()),
            Fragment::Literal(", %rsp\n    movq %rsp, %r14\n    leaq "),
            Fragment::Number(slot_bytes),
            Fragment::Literal("(%rsp), %r15\n"),
        ],
    );
    let body = call(
        &mut main,
        "control-graph-nodes",
        vec![
            input(0),
            input(1),
            input_ports.clone(),
            nodes,
            cached_edges.clone(),
            cached_slots.clone(),
            ordered,
        ],
        SemanticType::Text,
    );
    let gather = call(
        &mut main,
        "control-graph-outputs",
        vec![
            input(0),
            cached_slots,
            cached_edges,
            input_ports,
            output_ports,
        ],
        SemanticType::Text,
    );
    let result = call(
        &mut main,
        "control-loop-result",
        vec![outputs.clone()],
        SemanticType::Text,
    );
    let validate = call(
        &mut main,
        "control-graph-result",
        vec![input(0), input(1), outputs],
        SemanticType::Text,
    );
    let footer = template(
        &mut main,
        vec![
            Fragment::Text(result),
            Fragment::Text(validate),
            Fragment::Literal("    movq %rax, %r13\n    jmp "),
            Fragment::Text(leave.clone()),
            Fragment::Literal("\n"),
            Fragment::Text(fail),
            Fragment::Literal(":\n    xorq %r13, %r13\n"),
            Fragment::Text(leave),
            Fragment::Literal(
                ":\n    movq %r12, %rdi\n    call g0_native_leave\n    movq %r13, %rax\n    addq $",
            ),
            Fragment::Number(allocation),
            Fragment::Literal(", %rsp\n    jmp "),
            Fragment::Text(return_label.clone()),
            Fragment::Literal("\n"),
            Fragment::Text(early),
            Fragment::Literal(":\n    xorq %rax, %rax\n"),
            Fragment::Text(return_label),
            Fragment::Literal(
                ":\n    popq %r15\n    popq %r14\n    popq %r13\n    popq %r12\n    popq %rbp\n    ret\n.size ",
            ),
            Fragment::Text(symbol.clone()),
            Fragment::Literal(", .-"),
            Fragment::Text(symbol),
            Fragment::Literal("\n"),
        ],
    );
    let code = template(
        &mut main,
        vec![
            Fragment::Text(prologue),
            Fragment::Text(body),
            Fragment::Text(gather),
            Fragment::Text(footer),
        ],
    );
    vec![main.finish(vec![code])]
}
fn program_emission_graphs() -> Vec<Graph> {
    let state = vec![SemanticType::Bytes, rows(), int(), texts()];
    let mut cond = G::new(
        "control-program-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(1));
    let test = cond.compare(Operation::Lt, input(2), len);
    let mut body = G::new("control-program-body", state.clone(), state.clone());
    let descriptor = body.index(input(1), input(2));
    let start = body.field(descriptor.clone(), 0);
    let end = body.field(descriptor.clone(), 1);
    let len = body.arithmetic(Operation::Sub, end, start.clone());
    let bytes = body.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (start.clone(), int()),
            (len, int()),
        ],
        SemanticType::Bytes,
    );
    let zero = body.n(0);
    let mut local = vec![zero];
    for i in 1..7 {
        let offset = body.field(descriptor.clone(), i);
        local.push(body.arithmetic(Operation::Sub, offset, start.clone()));
    }
    let local = array(&mut body, local, seq());
    let code = call(
        &mut body,
        "control-graph",
        vec![bytes, input(2), local],
        SemanticType::Text,
    );
    let parts = append(&mut body, input(3), code, texts());
    let next = body.advance(input(2), 1);
    let mut main = G::new(
        "control-compile",
        vec![SemanticType::Bytes],
        vec![SemanticType::Text],
    );
    let descriptors = call(&mut main, "reader-program-ast", vec![input(0)], rows());
    let zero = main.n(0);
    let empty = array(&mut main, vec![], texts());
    let out = main.loop_node(
        "control-program",
        state,
        vec![input(0), descriptors, zero, empty],
    );
    let graph_code = join(&mut main, out[3].clone());
    let name_at = main.n(8);
    let entry = call(
        &mut main,
        "control-symbol",
        vec![input(0), name_at],
        SemanticType::Text,
    );
    let length = main.length(input(0));
    let bridge = template(
        &mut main,
        vec![
            Fragment::Literal(
                ".text\n.globl g0_compiled_entry\ng0_compiled_entry:\n    xorl %r9d, %r9d\n    movq %rsi, %rcx\n    movq %rdi, %rdx\n    leaq ",
            ),
            Fragment::Text(entry.clone()),
            Fragment::Literal("(%rip), %r8\n    leaq .Lg0_program(%rip), %rdi\n    movq $"),
            Fragment::Number(length.clone()),
            Fragment::Literal(
                ", %rsi\n    jmp g0_native_invoke\n.globl g0_compiled_entry_with_limits\ng0_compiled_entry_with_limits:\n    movq %rdx, %r9\n    movq %rsi, %rcx\n    movq %rdi, %rdx\n    leaq ",
            ),
            Fragment::Text(entry),
            Fragment::Literal("(%rip), %r8\n    leaq .Lg0_program(%rip), %rdi\n    movq $"),
            Fragment::Number(length),
            Fragment::Literal(
                ", %rsi\n    jmp g0_native_invoke\n.section .rodata\n.Lg0_program:\n.byte ",
            ),
        ],
    );
    let data = call(
        &mut main,
        "emit-program-byte-values",
        vec![input(0)],
        SemanticType::Text,
    );
    let code = template(
        &mut main,
        vec![
            Fragment::Text(graph_code),
            Fragment::Text(bridge),
            Fragment::Text(data),
            Fragment::Literal("\n.section .note.GNU-stack,\"\",@progbits\n"),
        ],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), next, parts]),
        main.finish(vec![code]),
    ]
}

fn port_rank_graphs() -> Vec<Graph> {
    let state = vec![
        SemanticType::Bytes,
        int(),
        int(),
        int(),
        int(),
        SemanticType::Bool,
    ];
    let mut condition = G::new(
        "control-port-rank-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = condition.n(0);
    let remaining = condition.compare(Operation::Gt, input(2), zero);
    let unseen = condition.not(input(5));
    let test = condition.and(remaining, unseen);
    let mut body = G::new("control-port-rank-body", state.clone(), state.clone());
    let id = mod_id(&mut body, input(1));
    let found = body.compare(Operation::Eq, id, input(3));
    let body_at = body.call("reader-port-layout", input(1));
    let one = body.n(1);
    let left = body.arithmetic(Operation::Sub, input(2), one);
    let next = body.advance(input(4), 1);
    let rank = body.pick("reader-pick-offset", found.clone(), next, input(4));
    let mut main = G::new(
        "control-port-rank",
        vec![SemanticType::Bytes, int(), int()],
        vec![int()],
    );
    let count = main.u32(input(1));
    let start = main.advance(input(1), 4);
    let zero = main.n(0);
    let no = main.bool(false);
    let out = main.loop_node(
        "control-port-rank",
        state,
        vec![input(0), start, count, input(2), zero, no],
    );
    vec![
        condition.finish(vec![test]),
        body.finish(vec![input(0), body_at, left, input(3), rank, found]),
        main.finish(vec![out[4].clone()]),
    ]
}
fn slot_graphs() -> Vec<Graph> {
    let state = vec![SemanticType::Bytes, int(), int(), int(), int(), rows()];
    let mut cond = G::new(
        "control-append-ports-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let test = cond.compare(Operation::Gt, input(2), zero);
    let mut body = G::new("control-append-ports-body", state.clone(), state.clone());
    let port_id = mod_id(&mut body, input(1));
    let row = array(&mut body, vec![input(3), port_id, input(4)], seq());
    let table = append(&mut body, input(5), row, rows());
    let body_at = body.call("reader-port-layout", input(1));
    let one = body.n(1);
    let left = body.arithmetic(Operation::Sub, input(2), one);
    let slot = body.advance(input(4), 1);
    let mut main = G::new(
        "control-append-ports",
        vec![SemanticType::Bytes, int(), int(), int(), rows()],
        vec![int(), rows()],
    );
    let count = main.u32(input(1));
    let at = main.advance(input(1), 4);
    let out = main.loop_node(
        "control-append-ports",
        state,
        vec![input(0), at, count, input(2), input(3), input(4)],
    );
    let mut graphs = vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), body_at, left, input(3), slot, table]),
        main.finish(vec![out[4].clone(), out[5].clone()]),
    ];
    let state = vec![SemanticType::Bytes, rows(), int(), int(), rows()];
    let mut cond = G::new(
        "control-node-slots-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(1));
    let test = cond.compare(Operation::Lt, input(2), len);
    let mut body = G::new("control-node-slots-body", state.clone(), state.clone());
    let descriptor = body.index(input(1), input(2));
    let id = body.field(descriptor.clone(), 0);
    let ports = body.field(descriptor, 4);
    let body_out = multi(
        &mut body,
        "control-append-ports",
        vec![input(0), ports, id, input(3), input(4)],
        vec![int(), rows()],
    );
    let next = body.advance(input(2), 1);
    let mut main = G::new(
        "control-node-slots",
        vec![SemanticType::Bytes, rows()],
        vec![int(), rows()],
    );
    let zero = main.n(0);
    let table = array(&mut main, vec![], rows());
    let out = main.loop_node(
        "control-node-slots",
        state,
        vec![input(0), input(1), zero.clone(), zero, table],
    );
    graphs.extend([
        cond.finish(vec![test]),
        body.finish(vec![
            input(0),
            input(1),
            next,
            body_out[0].clone(),
            body_out[1].clone(),
        ]),
        main.finish(vec![out[3].clone(), out[4].clone()]),
    ]);
    let state = vec![rows(), int(), int(), int(), int(), SemanticType::Bool];
    let mut cond = G::new(
        "control-slot-index-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(0));
    let before = cond.compare(Operation::Lt, input(3), len);
    let no = cond.not(input(5));
    let test = cond.and(before, no);
    let mut body = G::new("control-slot-index-body", state.clone(), state.clone());
    let row = body.index(input(0), input(3));
    let id = body.field(row.clone(), 0);
    let port_id = body.field(row.clone(), 1);
    let same_node = body.compare(Operation::Eq, id, input(1));
    let same_port = body.compare(Operation::Eq, port_id, input(2));
    let found = body.and(same_node, same_port);
    let slot = body.field(row, 2);
    let slot = body.pick("reader-pick-offset", found.clone(), input(4), slot);
    let next = body.advance(input(3), 1);
    let mut main = G::new(
        "control-slot-index",
        vec![rows(), int(), int()],
        vec![int()],
    );
    let zero = main.n(0);
    let no = main.bool(false);
    let out = main.loop_node(
        "control-slot-index",
        state,
        vec![input(0), input(1), input(2), zero.clone(), zero, no],
    );
    graphs.extend([
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), input(2), next, slot, found]),
        main.finish(vec![out[4].clone()]),
    ]);
    graphs
}
fn symbol_graphs() -> Vec<Graph> {
    let mut byte = G::new(
        "control-symbol-byte",
        vec![SemanticType::Integer(IntegerType { min: 0, max: 255 })],
        vec![SemanticType::Text],
    );
    let number = byte.number(input(0));
    let suffix = byte.text("_");
    let value = byte.concat(number, suffix);
    let mut main = G::new(
        "control-symbol",
        vec![SemanticType::Bytes, int()],
        vec![SemanticType::Text],
    );
    let len = main.u32(input(1));
    let start = main.advance(input(1), 4);
    let bytes = main.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (start, int()),
            (len, int()),
        ],
        SemanticType::Bytes,
    );
    let parts = main.op(
        Operation::Map {
            body: "control-symbol-byte".into(),
        },
        vec![(bytes, SemanticType::Bytes)],
        texts(),
    );
    let empty = main.text("");
    let joined = main.op(
        Operation::TextJoin,
        vec![(parts, texts()), (empty, SemanticType::Text)],
        SemanticType::Text,
    );
    let prefix = main.text("g0_direct_name_");
    let result = main.concat(prefix, joined);
    vec![byte.finish(vec![value]), main.finish(vec![result])]
}

pub(super) fn graphs() -> Vec<Graph> {
    let mut graphs = port_rank_graphs();
    graphs.extend(slot_graphs());
    graphs.extend(symbol_graphs());
    graphs.extend(argument_graphs());
    graphs.extend(operation_graphs());
    graphs.extend(output_graphs());
    graphs.extend(loop_graphs());
    graphs.extend(map_graphs());
    graphs.extend(match_graphs());
    graphs.extend(graph_output_graphs());
    graphs.extend(max_args_graphs());
    graphs.extend(graph_node_graphs());
    graphs.extend(graph_finish_graphs());
    graphs.extend(graph_emission_graphs());
    graphs.extend(program_emission_graphs());
    graphs.extend(domain_graphs());
    graphs
}

fn domain_graphs() -> Vec<Graph> {
    let port_state = vec![
        SemanticType::Bytes,
        int(),
        int(),
        int(),
        SemanticType::Bool,
        SemanticType::Bool,
    ];
    let mut pc = G::new(
        "control-domain-ports-condition",
        port_state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = pc.n(0);
    let test = pc.compare(Operation::Gt, input(2), zero);
    let mut pb = G::new(
        "control-domain-ports-body",
        port_state.clone(),
        port_state.clone(),
    );
    let port_id = mod_id(&mut pb, input(1));
    let ordered = pb.compare(Operation::Gt, port_id.clone(), input(3));
    let ordered = pb.logic(Operation::Or, ordered, input(5));
    let port_valid = pb.and(input(4), ordered);
    let name_at = pb.advance(input(1), 2);
    let type_at = pb.skip_blob(name_at);
    let supported = pb.op(
        Operation::Subgraph("validator-type-profile".into()),
        vec![(input(0), SemanticType::Bytes), (type_at, int())],
        SemanticType::Bool,
    );
    let port_valid = pb.and(port_valid, supported);
    let port_next = pb.call("reader-port-layout", input(1));
    let one = pb.n(1);
    let left = pb.arithmetic(Operation::Sub, input(2), one);
    let no = pb.bool(false);
    let mut ports = G::new(
        "control-domain-ports",
        vec![SemanticType::Bytes, int()],
        vec![SemanticType::Bool],
    );
    let count = ports.u32(input(1));
    let start = ports.advance(input(1), 4);
    let zero = ports.n(0);
    let yes = ports.bool(true);
    let out = ports.loop_node(
        "control-domain-ports",
        port_state,
        vec![input(0), start, count, zero, yes.clone(), yes],
    );
    let node_state = vec![
        SemanticType::Bytes,
        rows(),
        int(),
        int(),
        SemanticType::Bool,
    ];
    let mut nc = G::new(
        "control-domain-nodes-condition",
        node_state.clone(),
        vec![SemanticType::Bool],
    );
    let len = nc.length(input(1));
    let test_nodes = nc.compare(Operation::Lt, input(2), len);
    let mut nb = G::new(
        "control-domain-nodes-body",
        node_state.clone(),
        node_state.clone(),
    );
    let descriptor = nb.index(input(1), input(2));
    let id = nb.field(descriptor.clone(), 0);
    let zero = nb.n(0);
    let first = nb.compare(Operation::Eq, input(2), zero.clone());
    let after = nb.compare(Operation::Gt, id.clone(), input(3));
    let ordered = nb.logic(Operation::Or, first, after);
    let input_ports = nb.field(descriptor.clone(), 3);
    let output_ports = nb.field(descriptor.clone(), 4);
    let inputs_ok = call(
        &mut nb,
        "control-domain-ports",
        vec![input(0), input_ports],
        SemanticType::Bool,
    );
    let outputs_ok = call(
        &mut nb,
        "control-domain-ports",
        vec![input(0), output_ports],
        SemanticType::Bool,
    );
    let effects = nb.field(descriptor.clone(), 5);
    let caps = nb.field(descriptor.clone(), 6);
    let effects = nb.u32(effects);
    let caps = nb.u32(caps);
    let effects_ok = nb.compare(Operation::Eq, effects, zero.clone());
    let caps_ok = nb.compare(Operation::Eq, caps, zero);
    let op = nb.field(descriptor, 2);
    let tag = nb.byte(op);
    let seven = nb.n(7);
    let early = nb.compare(Operation::Le, tag.clone(), seven);
    let seventeen = nb.n(17);
    let forty_eight = nb.n(48);
    let middle_start = nb.compare(Operation::Ge, tag.clone(), seventeen);
    let middle_end = nb.compare(Operation::Le, tag.clone(), forty_eight);
    let middle = nb.and(middle_start, middle_end);
    let sixty_six = nb.n(66);
    let seventy_three = nb.n(74);
    let late_start = nb.compare(Operation::Ge, tag.clone(), sixty_six);
    let late_end = nb.compare(Operation::Le, tag, seventy_three);
    let late = nb.and(late_start, late_end);
    let early_middle = nb.logic(Operation::Or, early, middle);
    let allowed = nb.logic(Operation::Or, early_middle, late);
    let mut valid_node = input(4);
    for flag in [ordered, inputs_ok, outputs_ok, effects_ok, caps_ok, allowed] {
        valid_node = nb.and(valid_node, flag);
    }
    let next_node = nb.advance(input(2), 1);
    let mut nodes = G::new(
        "control-domain-nodes",
        vec![SemanticType::Bytes, rows()],
        vec![SemanticType::Bool],
    );
    let zero = nodes.n(0);
    let yes = nodes.bool(true);
    let node_out = nodes.loop_node(
        "control-domain-nodes",
        node_state,
        vec![input(0), input(1), zero.clone(), zero, yes],
    );
    let mut graph = G::new(
        "control-domain-graph",
        vec![SemanticType::Bytes, seq()],
        vec![SemanticType::Bool],
    );
    let zero = graph.n(0);
    let ast = call(&mut graph, "reader-graph-ast", vec![input(0), zero], rows());
    let node_valid = call(
        &mut graph,
        "control-domain-nodes",
        vec![input(0), ast],
        SemanticType::Bool,
    );
    let input_list = graph.field(input(1), 3);
    let output_list = graph.field(input(1), 4);
    let inputs_valid = call(
        &mut graph,
        "control-domain-ports",
        vec![input(0), input_list],
        SemanticType::Bool,
    );
    let outputs_valid = call(
        &mut graph,
        "control-domain-ports",
        vec![input(0), output_list],
        SemanticType::Bool,
    );
    let graph_valid = graph.and(node_valid, inputs_valid);
    let graph_valid = graph.and(graph_valid, outputs_valid);
    let program_state = vec![SemanticType::Bytes, rows(), int(), SemanticType::Bool];
    let mut cond = G::new(
        "control-domain-program-condition",
        program_state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(1));
    let test_program = cond.compare(Operation::Lt, input(2), len);
    let mut body = G::new(
        "control-domain-program-body",
        program_state.clone(),
        program_state.clone(),
    );
    let descriptor = body.index(input(1), input(2));
    let start = body.field(descriptor.clone(), 0);
    let end = body.field(descriptor.clone(), 1);
    let len = body.arithmetic(Operation::Sub, end, start.clone());
    let bytes = body.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (start.clone(), int()),
            (len, int()),
        ],
        SemanticType::Bytes,
    );
    let zero = body.n(0);
    let mut local = vec![zero];
    for i in 1..7 {
        let offset = body.field(descriptor.clone(), i);
        local.push(body.arithmetic(Operation::Sub, offset, start.clone()));
    }
    let local = array(&mut body, local, seq());
    let valid = call(
        &mut body,
        "control-domain-graph",
        vec![bytes, local],
        SemanticType::Bool,
    );
    let program_valid = body.and(input(3), valid);
    let next = body.advance(input(2), 1);
    let mut main = G::new(
        "control-domain",
        vec![SemanticType::Bytes],
        vec![SemanticType::Bool],
    );
    let descriptors = call(&mut main, "reader-program-ast", vec![input(0)], rows());
    let zero = main.n(0);
    let yes = main.bool(true);
    let program_out = main.loop_node(
        "control-domain-program",
        program_state,
        vec![input(0), descriptors, zero, yes],
    );
    vec![
        pc.finish(vec![test]),
        pb.finish(vec![input(0), port_next, left, port_id, port_valid, no]),
        ports.finish(vec![out[4].clone()]),
        nc.finish(vec![test_nodes]),
        nb.finish(vec![input(0), input(1), next_node, id, valid_node]),
        nodes.finish(vec![node_out[4].clone()]),
        graph.finish(vec![graph_valid]),
        cond.finish(vec![test_program]),
        body.finish(vec![input(0), input(1), next, program_valid]),
        main.finish(vec![program_out[3].clone()]),
    ]
}
