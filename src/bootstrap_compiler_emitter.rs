//! Native instructions are emitted by GIR graphs over the reader's offset AST.
//! The host only constructs these definitions; no source document reaches Rust
//! code in this module.
use super::*;
fn seq() -> SemanticType {
    SemanticType::Slice(Box::new(int()))
}
fn nodes() -> SemanticType {
    SemanticType::Slice(Box::new(seq()))
}
impl G {
    pub(super) fn text(&mut self, s: &str) -> SourceEndpoint {
        self.op(
            Operation::Const(Literal::Text(s.into())),
            vec![],
            SemanticType::Text,
        )
    }
    pub(super) fn concat(&mut self, a: SourceEndpoint, b: SourceEndpoint) -> SourceEndpoint {
        self.op(
            Operation::TextConcat,
            vec![(a, SemanticType::Text), (b, SemanticType::Text)],
            SemanticType::Text,
        )
    }
    pub(super) fn number(&mut self, a: SourceEndpoint) -> SourceEndpoint {
        let ty = self.ty(&a);
        self.op(Operation::FormatInteger, vec![(a, ty)], SemanticType::Text)
    }
    pub(super) fn line(
        &mut self,
        prefix: &str,
        value: SourceEndpoint,
        suffix: &str,
    ) -> SourceEndpoint {
        let prefix = self.text(prefix);
        let number = self.number(value);
        let line = self.concat(prefix, number);
        let suffix = self.text(suffix);
        self.concat(line, suffix)
    }
}
fn graph_start(g: &mut G) -> (SourceEndpoint, SourceEndpoint) {
    let eight = g.n(8);
    let entry_end = g.call("reader-string", eight);
    let count = g.u32(entry_end.clone());
    let start = g.advance(entry_end, 8);
    (start, count)
}

fn index_graphs() -> Vec<Graph> {
    let state = vec![nodes(), int(), int(), SemanticType::Bool];
    let mut cond = G::new(
        "emitter-node-index-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(0));
    let before = cond.compare(Operation::Lt, input(2), len);
    let unseen = cond.not(input(3));
    let test = cond.and(before, unseen);
    let mut body = G::new("emitter-node-index-body", state.clone(), state.clone());
    let candidate = body.index(input(0), input(2));
    let id = body.field(candidate, 0);
    let found = body.compare(Operation::Eq, id, input(1));
    let next = body.advance(input(2), 1);
    let next = body.pick("reader-pick-offset", found.clone(), next, input(2));
    let mut main = G::new("emitter-node-index", vec![nodes(), int()], vec![int()]);
    let zero = main.n(0);
    let no = main.bool(false);
    let out = main.loop_node(
        "emitter-node-index",
        state,
        vec![input(0), input(1), zero, no],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), next, found]),
        main.finish(vec![out[2].clone()]),
    ]
}
fn source_load(g: &mut G, canonical: SourceEndpoint, edge: SourceEndpoint) -> SourceEndpoint {
    let kind = g.field(edge.clone(), 0);
    let id = g.field(edge.clone(), 1);
    let port = g.field(edge, 2);
    let zero = g.n(0);
    let graph_input = g.compare(Operation::Eq, kind, zero);
    let eight = g.n(8);
    let input_offset = g.arithmetic(Operation::Mul, port, eight);
    let from_input = g.line("    movq ", input_offset, "(%r13), %rax\n");
    let index = g.op(
        Operation::Subgraph("emitter-node-index".into()),
        vec![(canonical, nodes()), (id, int())],
        int(),
    );
    let eight = g.n(8);
    let node_offset = g.arithmetic(Operation::Mul, index, eight);
    let from_node = g.line("    movq ", node_offset, "(%r14), %rax\n");
    g.pick("emitter-pick-text", graph_input, from_node, from_input)
}
fn argument_graphs() -> Vec<Graph> {
    let state = vec![nodes(), nodes(), int(), int(), SemanticType::Text];
    let mut cond = G::new(
        "emitter-arguments-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(1));
    let test = cond.compare(Operation::Lt, input(3), len);
    let mut body = G::new("emitter-arguments-body", state.clone(), state.clone());
    let edge = body.index(input(1), input(3));
    let target_kind = body.field(edge.clone(), 3);
    let target_id = body.field(edge.clone(), 4);
    let target_port = body.field(edge.clone(), 5);
    let zero = body.n(0);
    let targets_node = body.compare(Operation::Eq, target_kind, zero);
    let targets_this = body.compare(Operation::Eq, target_id, input(2));
    let needed = body.and(targets_node, targets_this);
    let code = source_load(&mut body, input(0), edge);
    let eight = body.n(8);
    let offset = body.arithmetic(Operation::Mul, target_port, eight);
    let store = body.line("    movq %rax, ", offset, "(%r15)\n");
    let code = body.concat(code, store);
    let code = body.concat(input(4), code);
    let code = body.pick("emitter-pick-text", needed, input(4), code);
    let next = body.advance(input(3), 1);
    let mut main = G::new(
        "emitter-arguments",
        vec![nodes(), nodes(), int()],
        vec![SemanticType::Text],
    );
    let zero = main.n(0);
    let empty = main.text("");
    let out = main.loop_node(
        "emitter-arguments",
        state,
        vec![input(0), input(1), input(2), zero, empty],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), input(2), next, code]),
        main.finish(vec![out[4].clone()]),
    ]
}
fn node_graphs() -> Vec<Graph> {
    let state = vec![
        SemanticType::Bytes,
        nodes(),
        nodes(),
        nodes(),
        int(),
        SemanticType::Text,
    ];
    let mut cond = G::new(
        "emitter-nodes-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(3));
    let test = cond.compare(Operation::Lt, input(4), len);
    let mut body = G::new("emitter-nodes-body", state.clone(), state.clone());
    let descriptor = body.index(input(3), input(4));
    let id = body.field(descriptor.clone(), 0);
    let ports = body.field(descriptor, 3);
    let count = body.u32(ports);
    let code = body.op(
        Operation::Subgraph("emitter-arguments".into()),
        vec![
            (input(1), nodes()),
            (input(2), nodes()),
            (id.clone(), int()),
        ],
        SemanticType::Text,
    );
    let index = body.op(
        Operation::Subgraph("emitter-node-index".into()),
        vec![(input(1), nodes()), (id, int())],
        int(),
    );
    let setup = body.line(
        "    movq %r12, %rdi\n    xorl %esi, %esi\n    movq $",
        index.clone(),
        ", %rdx\n    movq %r15, %rcx\n    movq $",
    );
    let code = body.concat(code, setup);
    let setup = body.line("", count, ", %r8\n    call g0_native_primitive\n");
    let code = body.concat(code, setup);
    let eight = body.n(8);
    let offset = body.arithmetic(Operation::Mul, index, eight);
    let store = body.line("    movq %rax, ", offset, "(%r14)\n");
    let code = body.concat(code, store);
    let text = body.concat(input(5), code);
    let next = body.advance(input(4), 1);
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), input(2), input(3), next, text]),
    ]
}
fn output_graphs() -> Vec<Graph> {
    let state = vec![nodes(), nodes(), int(), SemanticType::Text];
    let mut cond = G::new(
        "emitter-output-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(1));
    let test = cond.compare(Operation::Lt, input(2), len);
    let mut body = G::new("emitter-output-body", state.clone(), state.clone());
    let edge = body.index(input(1), input(2));
    let kind = body.field(edge.clone(), 3);
    let one = body.n(1);
    let is_output = body.compare(Operation::Eq, kind, one);
    let code = source_load(&mut body, input(0), edge);
    let code = body.pick("emitter-pick-text", is_output, input(3), code);
    let next = body.advance(input(2), 1);
    let mut main = G::new(
        "emitter-output",
        vec![nodes(), nodes()],
        vec![SemanticType::Text],
    );
    let zero = main.n(0);
    let empty = main.text("");
    let out = main.loop_node(
        "emitter-output",
        state,
        vec![input(0), input(1), zero, empty],
    );
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), next, code]),
        main.finish(vec![out[3].clone()]),
    ]
}
pub(super) fn graphs() -> Vec<Graph> {
    let mut graphs = Vec::new();
    for (suffix, p) in [("first", 0), ("second", 1)] {
        let g = G::new(
            &format!("emitter-pick-text-{suffix}"),
            vec![SemanticType::Text, SemanticType::Text],
            vec![SemanticType::Text],
        );
        graphs.push(g.finish(vec![input(p)]));
    }
    graphs.extend(index_graphs());
    graphs.extend(argument_graphs());
    graphs.extend(node_graphs());
    graphs.extend(output_graphs());
    graphs.extend(domain_graphs());
    let mut main = G::new(
        "compile-direct-scalar",
        vec![SemanticType::Bytes],
        vec![SemanticType::Text],
    );
    let (start, _count) = graph_start(&mut main);
    let canonical = main.op(
        Operation::Subgraph("reader-graph-ast".into()),
        vec![(input(0), SemanticType::Bytes), (start.clone(), int())],
        nodes(),
    );
    let edges = main.op(
        Operation::Subgraph("reader-graph-edges".into()),
        vec![(input(0), SemanticType::Bytes), (start, int())],
        nodes(),
    );
    let ordered = main.op(
        Operation::Subgraph("scheduler-order".into()),
        vec![(canonical.clone(), nodes()), (edges.clone(), nodes())],
        nodes(),
    );
    let count = main.length(canonical.clone());
    let count = main.op(
        Operation::ConvertChecked,
        vec![(
            count,
            SemanticType::Integer(IntegerType {
                min: 0,
                max: u64::MAX as i128,
            }),
        )],
        int(),
    );
    let eight = main.n(8);
    let slots = main.arithmetic(Operation::Mul, count, eight);
    let allocation = main.advance(slots.clone(), 271);
    let sixteen = main.n(16);
    let allocation = main.op(
        Operation::Div,
        vec![(allocation, int()), (sixteen, int())],
        int(),
    );
    let sixteen = main.n(16);
    let allocation = main.arithmetic(Operation::Mul, allocation, sixteen);
    let prologue=main.line(".text\n.globl g0_direct_entry\n.type g0_direct_entry, @function\ng0_direct_entry:\n    pushq %rbp\n    movq %rsp, %rbp\n    pushq %r12\n    pushq %r13\n    pushq %r14\n    pushq %r15\n    subq $",allocation.clone(),", %rsp\n    movq %rdi, %r12\n    movq %rsi, %r13\n    movq %rsp, %r14\n    leaq ");
    let number = main.number(slots);
    let prologue = main.concat(prologue, number);
    let suffix=main.text("(%rsp), %r15\n    movq %rdx, %rcx\n    movq %r13, %rdx\n    xorl %esi, %esi\n    call g0_native_graph_enter\n");
    let prologue = main.concat(prologue, suffix);
    let zero = main.n(0);
    let state = vec![
        SemanticType::Bytes,
        nodes(),
        nodes(),
        nodes(),
        int(),
        SemanticType::Text,
    ];
    let out = main.loop_node(
        "emitter-nodes",
        state,
        vec![
            input(0),
            canonical.clone(),
            edges.clone(),
            ordered,
            zero,
            prologue,
        ],
    );
    let output = main.op(
        Operation::Subgraph("emitter-output".into()),
        vec![(canonical, nodes()), (edges, nodes())],
        SemanticType::Text,
    );
    let assembly = main.concat(out[5].clone(), output);
    let footer=main.text("    movq %rax, %rdx\n    movq %r12, %rdi\n    xorl %esi, %esi\n    call g0_native_graph_result\n    movq %rax, %r13\n    movq %r12, %rdi\n    call g0_native_leave\n    movq %r13, %rax\n");
    let assembly = main.concat(assembly, footer);
    let footer=main.line("    addq $",allocation,", %rsp\n    popq %r15\n    popq %r14\n    popq %r13\n    popq %r12\n    popq %rbp\n    ret\n.size g0_direct_entry, .-g0_direct_entry\n");
    let assembly = main.concat(assembly, footer);
    let length = main.length(input(0));
    let prefix=main.text(".globl g0_compiled_entry\ng0_compiled_entry:\n    xorl %r9d, %r9d\n    movq %rsi, %rcx\n    movq %rdi, %rdx\n    leaq g0_direct_entry(%rip), %r8\n    leaq .Lg0_program(%rip), %rdi\n    movq $");
    let number = main.number(length.clone());
    let bridge = main.concat(prefix, number);
    let suffix=main.text(", %rsi\n    jmp g0_native_invoke\n.globl g0_compiled_entry_with_limits\ng0_compiled_entry_with_limits:\n    movq %rdx, %r9\n    movq %rsi, %rcx\n    movq %rdi, %rdx\n    leaq g0_direct_entry(%rip), %r8\n    leaq .Lg0_program(%rip), %rdi\n    movq $");
    let bridge = main.concat(bridge, suffix);
    let number = main.number(length);
    let bridge = main.concat(bridge, number);
    let suffix =
        main.text(", %rsi\n    jmp g0_native_invoke\n.section .rodata\n.Lg0_program:\n.byte ");
    let bridge = main.concat(bridge, suffix);
    let assembly = main.concat(assembly, bridge);
    let lines_ty = SemanticType::Slice(Box::new(SemanticType::Text));
    let lines = main.op(
        Operation::Map {
            body: "emit-byte".into(),
        },
        vec![(input(0), SemanticType::Bytes)],
        lines_ty.clone(),
    );
    let empty = main.text("\n.byte ");
    let lines = main.op(
        Operation::TextJoin,
        vec![(lines, lines_ty), (empty, SemanticType::Text)],
        SemanticType::Text,
    );
    let assembly = main.concat(assembly, lines);
    let footer = main.text("\n.section .note.GNU-stack,\"\",@progbits\n");
    let assembly = main.concat(assembly, footer);
    graphs.push(main.finish(vec![assembly]));
    graphs
}

fn domain_graphs() -> Vec<Graph> {
    let mut graphs = Vec::new();
    let state = vec![SemanticType::Bytes, int(), int(), int(), SemanticType::Bool];
    let mut cond = G::new(
        "emitter-contiguous-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let test = cond.compare(Operation::Gt, input(2), zero);
    let mut body = G::new("emitter-contiguous-body", state.clone(), state.clone());
    let word = body.u32(input(1));
    let modulus = body.n(65536);
    let id = body.op(Operation::Rem, vec![(word, int()), (modulus, int())], int());
    let matches = body.compare(Operation::Eq, id, input(3));
    let valid = body.and(input(4), matches);
    let body_at = body.call("reader-port", input(1));
    let one = body.n(1);
    let left = body.arithmetic(Operation::Sub, input(2), one);
    let index = body.advance(input(3), 1);
    let mut main = G::new(
        "emitter-contiguous",
        vec![SemanticType::Bytes, int()],
        vec![SemanticType::Bool],
    );
    let count = main.u32(input(1));
    let at = main.advance(input(1), 4);
    let zero = main.n(0);
    let yes = main.bool(true);
    let out = main.loop_node(
        "emitter-contiguous",
        state,
        vec![input(0), at, count, zero, yes],
    );
    graphs.extend([
        cond.finish(vec![test]),
        body.finish(vec![input(0), body_at, left, index, valid]),
        main.finish(vec![out[4].clone()]),
    ]);
    let state = vec![SemanticType::Bytes, nodes(), int(), SemanticType::Bool];
    let mut cond = G::new(
        "emitter-domain-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(1));
    let test = cond.compare(Operation::Lt, input(2), len);
    let mut body = G::new("emitter-domain-body", state.clone(), state.clone());
    let descriptor = body.index(input(1), input(2));
    let operation = body.field(descriptor.clone(), 2);
    let tag = body.byte(operation);
    let three = body.n(3);
    let core = body.compare(Operation::Le, tag.clone(), three);
    let first = body.n(17);
    let low = body.compare(Operation::Ge, tag.clone(), first);
    let last = body.n(46);
    let high = body.compare(Operation::Le, tag.clone(), last);
    let primitive = body.and(low, high);
    let fortyeight = body.n(48);
    let join = body.compare(Operation::Eq, tag.clone(), fortyeight);
    let first = body.n(66);
    let collection = body.compare(Operation::Ge, tag, first);
    let valid = body.logic(Operation::Or, core, primitive);
    let valid = body.logic(Operation::Or, valid, join);
    let valid = body.logic(Operation::Or, valid, collection);
    let input_ports = body.field(descriptor.clone(), 3);
    let output_ports = body.field(descriptor, 4);
    let input_count = body.u32(input_ports.clone());
    let thirtytwo = body.n(32);
    let bounded = body.compare(Operation::Le, input_count, thirtytwo);
    let output_count = body.u32(output_ports.clone());
    let one = body.n(1);
    let single = body.compare(Operation::Eq, output_count, one);
    let contiguous_inputs = body.op(
        Operation::Subgraph("emitter-contiguous".into()),
        vec![(input(0), SemanticType::Bytes), (input_ports, int())],
        SemanticType::Bool,
    );
    let contiguous_outputs = body.op(
        Operation::Subgraph("emitter-contiguous".into()),
        vec![(input(0), SemanticType::Bytes), (output_ports, int())],
        SemanticType::Bool,
    );
    let valid = body.and(valid, bounded);
    let valid = body.and(valid, single);
    let valid = body.and(valid, contiguous_inputs);
    let valid = body.and(valid, contiguous_outputs);
    let body_valid = body.and(input(3), valid);
    let next = body.advance(input(2), 1);
    let mut main = G::new(
        "direct-domain-one",
        vec![SemanticType::Bytes],
        vec![SemanticType::Bool],
    );
    let (start, _count) = graph_start(&mut main);
    let name = main.advance(start.clone(), 8);
    let ports = main.call("reader-string", name);
    let inputs = main.advance(ports, 1);
    let outputs = main.call("reader-list-reader-port", inputs.clone());
    let graph_inputs = main.op(
        Operation::Subgraph("emitter-contiguous".into()),
        vec![(input(0), SemanticType::Bytes), (inputs, int())],
        SemanticType::Bool,
    );
    let graph_outputs = main.op(
        Operation::Subgraph("emitter-contiguous".into()),
        vec![(input(0), SemanticType::Bytes), (outputs.clone(), int())],
        SemanticType::Bool,
    );
    let count = main.u32(outputs);
    let one = main.n(1);
    let single = main.compare(Operation::Eq, count, one);
    let valid = main.and(graph_inputs, graph_outputs);
    let valid = main.and(valid, single);
    let ast = main.op(
        Operation::Subgraph("reader-graph-ast".into()),
        vec![(input(0), SemanticType::Bytes), (start, int())],
        nodes(),
    );
    let zero = main.n(0);
    let out = main.loop_node("emitter-domain", state, vec![input(0), ast, zero, valid]);
    graphs.extend([
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), next, body_valid]),
        main.finish(vec![out[3].clone()]),
    ]);
    let mut no = G::new(
        "direct-domain-no",
        vec![SemanticType::Bytes],
        vec![SemanticType::Bool],
    );
    let false_value = no.bool(false);
    graphs.push(no.finish(vec![false_value]));
    let mut main = G::new(
        "direct-domain",
        vec![SemanticType::Bytes],
        vec![SemanticType::Bool],
    );
    let (_start, count) = graph_start(&mut main);
    let one = main.n(1);
    let single = main.compare(Operation::Eq, count, one);
    let result = main.op(
        Operation::Select {
            when_true: "direct-domain-one".into(),
            when_false: "direct-domain-no".into(),
        },
        vec![
            (single, SemanticType::Bool),
            (input(0), SemanticType::Bytes),
        ],
        SemanticType::Bool,
    );
    let node = main.b.graph.nodes.last_mut().unwrap();
    node.inputs[0].name = "selector".into();
    node.inputs[1].name = "p0".into();
    graphs.push(main.finish(vec![result]));
    graphs
}
