//! The binary reader is expressed as executable GIR. Rust here only constructs
//! the definition; it never reads, decodes, or validates compiler input bytes.
use super::*;
#[path = "bootstrap_compiler_bytes.rs"]
mod bytes;
#[path = "bootstrap_compiler_callcycles.rs"]
mod callcycles;
#[path = "bootstrap_compiler_context.rs"]
mod context;
#[path = "bootstrap_compiler_control.rs"]
mod control;
#[path = "bootstrap_compiler_control_fast.rs"]
mod control_fast;
#[path = "bootstrap_compiler_controlcheck.rs"]
mod controlcheck;
#[path = "bootstrap_compiler_emitter.rs"]
mod emitter;
#[path = "bootstrap_compiler_lookup_fast.rs"]
mod lookup_fast;
#[path = "bootstrap_compiler_names_fast.rs"]
mod names_fast;
#[path = "bootstrap_compiler_nodeschema.rs"]
mod nodeschema;
#[path = "bootstrap_compiler_operationcheck.rs"]
mod operationcheck;
#[path = "bootstrap_compiler_schedule_fast.rs"]
mod schedule_fast;
#[path = "bootstrap_compiler_semantics.rs"]
mod semantics;

fn int() -> SemanticType {
    SemanticType::Integer(IntegerType {
        min: 0,
        max: 1_i128 << 48,
    })
}
pub(super) fn contextual_graphs(definitions: &[Graph]) -> Vec<Graph> {
    context::graphs(definitions)
}
fn input(n: u16) -> SourceEndpoint {
    SourceEndpoint::GraphInput(n)
}
struct G {
    b: Builder,
}
impl G {
    fn new(name: &str, inputs: Vec<SemanticType>, outputs: Vec<SemanticType>) -> Self {
        let mut b = Builder::new(name, SemanticType::Bytes);
        b.graph.inputs = inputs
            .into_iter()
            .enumerate()
            .map(|(i, t)| port(i as u16, t))
            .collect();
        b.graph.outputs = outputs
            .into_iter()
            .enumerate()
            .map(|(i, t)| port(i as u16, t))
            .collect();
        Self { b }
    }
    fn op(
        &mut self,
        op: Operation,
        args: Vec<(SourceEndpoint, SemanticType)>,
        ty: SemanticType,
    ) -> SourceEndpoint {
        self.b.op(op, args, ty)
    }
    fn ty(&self, e: &SourceEndpoint) -> SemanticType {
        match e {
            SourceEndpoint::GraphInput(p) => self.b.graph.inputs[*p as usize].ty.clone(),
            SourceEndpoint::NodeOutput { node, port } => self.b.graph.nodes[*node as usize - 1]
                .outputs[*port as usize]
                .ty
                .clone(),
        }
    }
    fn n(&mut self, n: i128) -> SourceEndpoint {
        self.op(
            Operation::Const(Literal::Integer(n)),
            vec![],
            SemanticType::Integer(IntegerType { min: n, max: n }),
        )
    }
    fn arithmetic(
        &mut self,
        op: Operation,
        a: SourceEndpoint,
        b: SourceEndpoint,
    ) -> SourceEndpoint {
        let at = self.ty(&a);
        let bt = self.ty(&b);
        let (SemanticType::Integer(x), SemanticType::Integer(y)) = (&at, &bt) else {
            panic!("integer inputs")
        };
        let (min, max) = match op {
            Operation::Add => (x.min + y.min, x.max + y.max),
            Operation::Sub => (x.min - y.max, x.max - y.min),
            Operation::Mul => (x.min * y.min, x.max * y.max),
            _ => panic!("arithmetic"),
        };
        let result = self.op(
            op,
            vec![(a, at), (b, bt)],
            SemanticType::Integer(IntegerType { min, max }),
        );
        self.op(
            Operation::ConvertChecked,
            vec![(result, SemanticType::Integer(IntegerType { min, max }))],
            int(),
        )
    }
    fn compare(&mut self, op: Operation, a: SourceEndpoint, b: SourceEndpoint) -> SourceEndpoint {
        let (SemanticType::Integer(x), SemanticType::Integer(y)) = (self.ty(&a), self.ty(&b))
        else {
            panic!("integer comparison")
        };
        let ty = SemanticType::Integer(IntegerType {
            min: x.min.min(y.min),
            max: x.max.max(y.max),
        });
        self.op(op, vec![(a, ty.clone()), (b, ty)], SemanticType::Bool)
    }
    fn and(&mut self, a: SourceEndpoint, b: SourceEndpoint) -> SourceEndpoint {
        self.op(
            Operation::And,
            vec![(a, SemanticType::Bool), (b, SemanticType::Bool)],
            SemanticType::Bool,
        )
    }
    fn u32(&mut self, at: SourceEndpoint) -> SourceEndpoint {
        self.op(
            Operation::Subgraph("reader-u32".into()),
            vec![(input(0), SemanticType::Bytes), (at, int())],
            int(),
        )
    }
    fn u32_definition(&mut self, at: SourceEndpoint) -> SourceEndpoint {
        let mut value = self.n(0);
        for offset in 0..4 {
            let delta = self.n(offset);
            let index = self.arithmetic(Operation::Add, at.clone(), delta);
            let byte_type = SemanticType::Integer(IntegerType { min: 0, max: 255 });
            let optional = SemanticType::Option(Box::new(byte_type.clone()));
            let byte = self.op(
                Operation::Index,
                vec![(input(0), SemanticType::Bytes), (index, int())],
                optional.clone(),
            );
            let zero = self.op(
                Operation::Const(Literal::Integer(0)),
                vec![],
                byte_type.clone(),
            );
            let byte = self.op(
                Operation::UnwrapOr,
                vec![(byte, optional), (zero, byte_type.clone())],
                byte_type.clone(),
            );
            // Each byte is in 0..=255. After k bytes the accumulator is in
            // 0..=2^(8*k)-1, so these operations fit both i128 and int().
            // The exact static intervals remove redundant checked narrowing.
            let weight = 1_i128 << (offset * 8);
            let power = self.n(weight);
            let power_type = self.ty(&power);
            let part_type = SemanticType::Integer(IntegerType {
                min: 0,
                max: 255 * weight,
            });
            let part = self.op(
                Operation::Mul,
                vec![(byte, byte_type), (power, power_type)],
                part_type.clone(),
            );
            let value_type = self.ty(&value);
            let sum_type = SemanticType::Integer(IntegerType {
                min: 0,
                max: (1_i128 << ((offset + 1) * 8)) - 1,
            });
            value = self.op(
                Operation::Add,
                vec![(value, value_type), (part, part_type)],
                sum_type,
            );
        }
        value
    }
    fn skip_blob(&mut self, at: SourceEndpoint) -> SourceEndpoint {
        let length = self.u32(at.clone());
        let four = self.n(4);
        let start = self.arithmetic(Operation::Add, at, four);
        self.arithmetic(Operation::Add, start, length)
    }
    fn advance(&mut self, at: SourceEndpoint, n: i128) -> SourceEndpoint {
        let n = self.n(n);
        self.arithmetic(Operation::Add, at, n)
    }
    fn call(&mut self, name: &str, at: SourceEndpoint) -> SourceEndpoint {
        self.op(
            Operation::Subgraph(name.into()),
            vec![(input(0), SemanticType::Bytes), (at, int())],
            int(),
        )
    }
    fn byte(&mut self, at: SourceEndpoint) -> SourceEndpoint {
        let ty = SemanticType::Integer(IntegerType { min: 0, max: 255 });
        let option = SemanticType::Option(Box::new(ty.clone()));
        let value = self.op(
            Operation::Index,
            vec![(input(0), SemanticType::Bytes), (at, int())],
            option.clone(),
        );
        let zero = self.op(Operation::Const(Literal::Integer(0)), vec![], ty.clone());
        self.op(
            Operation::UnwrapOr,
            vec![(value, option), (zero, ty.clone())],
            ty,
        )
    }
    fn guard(&mut self, valid: SourceEndpoint, at: SourceEndpoint) -> SourceEndpoint {
        let out = self.op(
            Operation::Select {
                when_true: "reader-identity".into(),
                when_false: "reader-invalid".into(),
            },
            vec![
                (valid, SemanticType::Bool),
                (input(0), SemanticType::Bytes),
                (at, int()),
            ],
            int(),
        );
        let node = self.b.graph.nodes.last_mut().unwrap();
        node.inputs[0].name = "selector".into();
        node.inputs[1].name = "p0".into();
        node.inputs[2].name = "p1".into();
        out
    }
    fn bool(&mut self, v: bool) -> SourceEndpoint {
        self.op(
            Operation::Const(Literal::Bool(v)),
            vec![],
            SemanticType::Bool,
        )
    }
    fn logic(&mut self, op: Operation, a: SourceEndpoint, b: SourceEndpoint) -> SourceEndpoint {
        self.op(
            op,
            vec![(a, SemanticType::Bool), (b, SemanticType::Bool)],
            SemanticType::Bool,
        )
    }
    fn not(&mut self, a: SourceEndpoint) -> SourceEndpoint {
        self.op(
            Operation::Not,
            vec![(a, SemanticType::Bool)],
            SemanticType::Bool,
        )
    }
    fn length(&mut self, a: SourceEndpoint) -> SourceEndpoint {
        let ty = self.ty(&a);
        self.op(
            Operation::Length,
            vec![(a, ty)],
            SemanticType::Integer(IntegerType {
                min: 0,
                max: u64::MAX as i128,
            }),
        )
    }
    fn index(&mut self, a: SourceEndpoint, index: SourceEndpoint) -> SourceEndpoint {
        let ty = self.ty(&a);
        let SemanticType::Slice(inner) = &ty else {
            panic!("slice index")
        };
        let inner = *inner.clone();
        let option = SemanticType::Option(Box::new(inner.clone()));
        let item = self.op(
            Operation::Index,
            vec![(a, ty), (index, int())],
            option.clone(),
        );
        let default = if matches!(inner, SemanticType::Slice(_)) {
            self.op(Operation::MakeArray, vec![], inner.clone())
        } else {
            self.op(Operation::Const(Literal::Integer(0)), vec![], inner.clone())
        };
        self.op(
            Operation::UnwrapOr,
            vec![(item, option), (default, inner.clone())],
            inner,
        )
    }
    fn field(&mut self, a: SourceEndpoint, n: i128) -> SourceEndpoint {
        let n = self.n(n);
        self.index(a, n)
    }
    fn loop_node(
        &mut self,
        name: &str,
        state: Vec<SemanticType>,
        args: Vec<SourceEndpoint>,
    ) -> Vec<SourceEndpoint> {
        let id = self.b.graph.nodes.len() as u32 + 1;
        self.op(
            Operation::Loop {
                condition: format!("{name}-condition"),
                body: format!("{name}-body"),
                max_iterations: 131072,
            },
            args.into_iter().zip(state.clone()).collect(),
            state[0].clone(),
        );
        self.b.graph.nodes.last_mut().unwrap().outputs = state
            .into_iter()
            .enumerate()
            .map(|(i, t)| port(i as u16, t))
            .collect();
        (0..self.b.graph.nodes.last().unwrap().outputs.len())
            .map(|p| SourceEndpoint::NodeOutput {
                node: id,
                port: p as u16,
            })
            .collect()
    }
    fn pick(
        &mut self,
        name: &str,
        test: SourceEndpoint,
        a: SourceEndpoint,
        b: SourceEndpoint,
    ) -> SourceEndpoint {
        let ty = if matches!(self.ty(&a), SemanticType::Integer(_)) {
            int()
        } else {
            self.ty(&a)
        };
        let out = self.op(
            Operation::Select {
                when_true: format!("{name}-second"),
                when_false: format!("{name}-first"),
            },
            vec![(test, SemanticType::Bool), (a, ty.clone()), (b, ty.clone())],
            ty,
        );
        let node = self.b.graph.nodes.last_mut().unwrap();
        node.inputs[0].name = "selector".into();
        node.inputs[1].name = "p0".into();
        node.inputs[2].name = "p1".into();
        out
    }
    fn finish(mut self, values: Vec<SourceEndpoint>) -> Graph {
        for (i, from) in values.into_iter().enumerate() {
            self.b.graph.edges.push(Edge {
                from,
                to: TargetEndpoint::GraphOutput(i as u16),
            });
        }
        self.b.graph
    }
}

pub(super) fn graphs() -> Vec<Graph> {
    let mut syntax = syntax_graphs();
    let mut word = G::new("reader-u32", vec![SemanticType::Bytes, int()], vec![int()]);
    let value = word.u32_definition(input(1));
    let word = word.finish(vec![value]);
    let state = vec![SemanticType::Bytes, int(), int()];
    let mut cond = G::new(
        "reader-blobs-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let test = cond.compare(Operation::Gt, input(2), zero);
    let cond = cond.finish(vec![test]);
    let mut body = G::new("reader-blobs-body", state.clone(), state.clone());
    let length = body.u32(input(1));
    let start = body.advance(input(1), 4);
    let expected = body.arithmetic(Operation::Add, start.clone(), length.clone());
    let bytes = body.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (start, int()),
            (length.clone(), int()),
        ],
        SemanticType::Bytes,
    );
    let zero = body.n(0);
    let actual = body.op(
        Operation::Subgraph("reader-graph".into()),
        vec![(bytes, SemanticType::Bytes), (zero, int())],
        int(),
    );
    let good = body.compare(Operation::Eq, actual, length);
    let next = body.op(
        Operation::Select {
            when_true: "reader-identity".into(),
            when_false: "reader-invalid".into(),
        },
        vec![
            (good, SemanticType::Bool),
            (input(0), SemanticType::Bytes),
            (expected, int()),
        ],
        int(),
    );
    body.b.graph.nodes.last_mut().unwrap().inputs[0].name = "selector".into();
    body.b.graph.nodes.last_mut().unwrap().inputs[1].name = "p0".into();
    body.b.graph.nodes.last_mut().unwrap().inputs[2].name = "p1".into();
    let one = body.n(1);
    let left = body.arithmetic(Operation::Sub, input(2), one);
    let body = body.finish(vec![input(0), next, left]);
    let mut main = G::new(
        "reader-container",
        vec![SemanticType::Bytes],
        vec![SemanticType::Bool],
    );
    let at = main.n(0);
    let magic = main.u32(at);
    let expected = main.n(0x00503047);
    let valid_magic = main.compare(Operation::Eq, magic, expected);
    let at = main.n(4);
    let version = main.u32(at);
    let expected = main.n(0x00030000);
    let valid_version = main.compare(Operation::Eq, version, expected);
    let header_valid = main.and(valid_magic, valid_version);
    let at = main.n(8);
    let entry_end = main.call("reader-string", at);
    let graph_count = main.u32(entry_end.clone());
    let four = main.n(4);
    let graph_start = main.arithmetic(Operation::Add, entry_end, four);
    // Loop nodes carry three values. The reader's cursor is output port one.
    let loop_id = main.b.graph.nodes.len() as u32 + 1;
    main.op(
        Operation::Loop {
            condition: "reader-blobs-condition".into(),
            body: "reader-blobs-body".into(),
            max_iterations: 131072,
        },
        vec![
            (input(0), SemanticType::Bytes),
            (graph_start, int()),
            (graph_count, int()),
        ],
        SemanticType::Bytes,
    );
    main.b.graph.nodes.last_mut().unwrap().outputs = state
        .into_iter()
        .enumerate()
        .map(|(i, t)| port(i as u16, t))
        .collect();
    let graph_end = SourceEndpoint::NodeOutput {
        node: loop_id,
        port: 1,
    };
    let end = main.call("reader-list-reader-schema", graph_end);
    let length = main.op(
        Operation::Length,
        vec![(input(0), SemanticType::Bytes)],
        SemanticType::Integer(IntegerType {
            min: 0,
            max: u64::MAX as i128,
        }),
    );
    let eof = main.compare(Operation::Eq, end, length);
    let valid = main.and(header_valid, eof);
    let container = main.finish(vec![valid]);
    let mut layout_condition = cond.clone();
    layout_condition.name = "reader-blobs-syntax-condition".into();
    let mut layout_body = body.clone();
    layout_body.name = "reader-blobs-syntax-body".into();
    for node in &mut layout_body.nodes {
        if let Operation::Subgraph(name) = &mut node.operation
            && name == "reader-graph"
        {
            *name = "reader-graph-syntax".into();
        }
    }
    let mut layout_container = container.clone();
    layout_container.name = "reader-container-syntax".into();
    for node in &mut layout_container.nodes {
        if let Operation::Loop {
            condition, body, ..
        } = &mut node.operation
            && condition == "reader-blobs-condition"
        {
            *condition = "reader-blobs-syntax-condition".into();
            *body = "reader-blobs-syntax-body".into();
        }
    }
    syntax.extend([
        word,
        cond,
        body,
        container,
        layout_condition,
        layout_body,
        layout_container,
    ]);
    syntax.extend(ast_graphs());
    syntax.extend(program_ast_graphs());
    syntax.extend(version_validation_graphs());
    syntax.extend(edge_ast_graphs());
    syntax.extend(scheduler_graphs());
    syntax.extend(emitter::graphs());
    syntax.extend(control::graphs());
    syntax.extend(control_fast::graphs());
    syntax.extend(semantics::graphs());
    syntax.extend(bytes::graphs());
    syntax.extend(schedule_fast::graphs());
    syntax.extend(lookup_fast::graphs());
    syntax.extend(controlcheck::graphs());
    syntax.extend(callcycles::graphs());
    syntax.extend(nodeschema::graphs());
    syntax.extend(names_fast::graphs());
    syntax.extend(operationcheck::graphs());
    syntax
}

#[derive(Clone)]
enum Part {
    Fixed(i128),
    Blob,
    Call(&'static str),
    Repeat(&'static str),
}
fn cursor_graph(name: &str) -> G {
    G::new(name, vec![SemanticType::Bytes, int()], vec![int()])
}
fn sequence(name: &str, parts: &[Part]) -> Graph {
    let mut g = cursor_graph(name);
    let mut at = input(1);
    for part in parts {
        at = match part {
            Part::Fixed(n) => g.advance(at, *n),
            Part::Blob => g.call("reader-string", at),
            Part::Call(name) => g.call(name, at),
            Part::Repeat(name) => g.call(&format!("reader-list-{name}"), at),
        };
    }
    g.finish(vec![at])
}
fn list_graphs(name: &str) -> Vec<Graph> {
    if matches!(name, "reader-port" | "reader-node-checked") {
        return unique_identifier_list_graphs(name);
    }
    let state = vec![SemanticType::Bytes, int(), int()];
    let mut cond = G::new(
        &format!("reader-list-{name}-condition"),
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let test = cond.compare(Operation::Gt, input(2), zero);
    let mut body = G::new(
        &format!("reader-list-{name}-body"),
        state.clone(),
        state.clone(),
    );
    let next = body.call(name, input(1));
    let one = body.n(1);
    let left = body.arithmetic(Operation::Sub, input(2), one);
    let mut main = cursor_graph(&format!("reader-list-{name}"));
    let count = main.u32(input(1));
    let start = main.advance(input(1), 4);
    let id = main.b.graph.nodes.len() as u32 + 1;
    main.op(
        Operation::Loop {
            condition: cond.b.graph.name.clone(),
            body: body.b.graph.name.clone(),
            max_iterations: 131072,
        },
        vec![
            (input(0), SemanticType::Bytes),
            (start, int()),
            (count, int()),
        ],
        SemanticType::Bytes,
    );
    main.b.graph.nodes.last_mut().unwrap().outputs = state
        .into_iter()
        .enumerate()
        .map(|(i, t)| port(i as u16, t))
        .collect();
    vec![
        cond.finish(vec![test]),
        body.finish(vec![input(0), next, left]),
        main.finish(vec![SourceEndpoint::NodeOutput { node: id, port: 1 }]),
    ]
}

fn unique_identifier_list_graphs(name: &str) -> Vec<Graph> {
    let ids = SemanticType::Slice(Box::new(int()));
    let state = vec![
        SemanticType::Bytes,
        int(),
        int(),
        ids.clone(),
        SemanticType::Bool,
        int(),
    ];
    let prefix = format!("reader-list-{name}");
    let mut condition = G::new(
        &format!("{prefix}-condition"),
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = condition.n(0);
    let test = condition.compare(Operation::Gt, input(2), zero);
    let test = condition.and(test, input(4));
    let mut body = G::new(&format!("{prefix}-body"), state.clone(), state.clone());
    let word = body.u32(input(1));
    let id = if name == "reader-port" {
        let modulus = body.n(65536);
        body.op(Operation::Rem, vec![(word, int()), (modulus, int())], int())
    } else {
        word
    };
    let above_maximum = body.compare(Operation::Gt, id.clone(), input(5));
    let maximum = body.pick(
        "reader-pick-offset",
        above_maximum.clone(),
        input(5),
        id.clone(),
    );
    let duplicate = body.op(
        Operation::Select {
            when_true: format!("{prefix}-above-maximum"),
            when_false: "scheduler-contains".into(),
        },
        vec![
            (above_maximum, SemanticType::Bool),
            (input(3), ids.clone()),
            (id.clone(), int()),
        ],
        SemanticType::Bool,
    );
    let node = body.b.graph.nodes.last_mut().unwrap();
    node.inputs[0].name = "selector".into();
    node.inputs[1].name = "p0".into();
    node.inputs[2].name = "p1".into();
    let mut fresh = G::new(
        &format!("{prefix}-above-maximum"),
        vec![ids.clone(), int()],
        vec![SemanticType::Bool],
    );
    let not_duplicate = fresh.bool(false);
    let unique = body.not(duplicate);
    let valid = body.and(input(4), unique);
    let singleton = body.op(Operation::MakeArray, vec![(id, int())], ids.clone());
    let seen = body.op(
        Operation::ArrayConcat,
        vec![(input(3), ids.clone()), (singleton, ids.clone())],
        ids.clone(),
    );
    let next = body.call(name, input(1));
    let one = body.n(1);
    let remaining = body.arithmetic(Operation::Sub, input(2), one);
    let mut main = cursor_graph(&prefix);
    let count = main.u32(input(1));
    let start = main.advance(input(1), 4);
    let empty = main.op(Operation::MakeArray, vec![], ids);
    let initially_valid = main.bool(true);
    let initial_maximum = main.n(0);
    let output = main.loop_node(
        &prefix,
        state,
        vec![
            input(0),
            start,
            count,
            empty,
            initially_valid,
            initial_maximum,
        ],
    );
    let end = main.guard(output[4].clone(), output[1].clone());
    vec![
        condition.finish(vec![test]),
        body.finish(vec![input(0), next, remaining, seen, valid, maximum]),
        fresh.finish(vec![not_duplicate]),
        main.finish(vec![end]),
    ]
}
/// A tag dispatcher is GIR Select nodes and graph calls, not a Rust input switch.
fn dispatch(name: &str, cases: &[(i128, &str)]) -> Vec<Graph> {
    let mut graphs = Vec::new();
    fn build(name: &str, cases: &[(i128, &str)], graphs: &mut Vec<Graph>) {
        let mut g = cursor_graph(name);
        let byte = g.byte(input(1));
        let (test, yes, no) = if cases.len() == 1 {
            let (tag, target) = cases[0];
            let expected = g.n(tag);
            let test = g.compare(Operation::Eq, byte, expected);
            let adapter = format!("{name}-take");
            let mut a = cursor_graph(&adapter);
            let at = a.advance(input(1), 1);
            let value = a.call(target, at);
            graphs.push(a.finish(vec![value]));
            (test, adapter, "reader-invalid".into())
        } else {
            let midpoint = cases.len() / 2;
            let expected = g.n(cases[midpoint].0);
            let test = g.compare(Operation::Lt, byte, expected);
            let yes = format!("{name}-l");
            let no = format!("{name}-r");
            build(&yes, &cases[..midpoint], graphs);
            build(&no, &cases[midpoint..], graphs);
            (test, yes, no)
        };
        let out = g.op(
            Operation::Select {
                when_true: yes,
                when_false: no,
            },
            vec![
                (test, SemanticType::Bool),
                (input(0), SemanticType::Bytes),
                (input(1), int()),
            ],
            int(),
        );
        let node = g.b.graph.nodes.last_mut().unwrap();
        node.inputs[0].name = "selector".into();
        node.inputs[1].name = "p0".into();
        node.inputs[2].name = "p1".into();
        graphs.push(g.finish(vec![out]));
    }
    build(name, cases, &mut graphs);
    graphs
}

fn syntax_graphs() -> Vec<Graph> {
    use Part::*;
    let mut invalid = cursor_graph("reader-invalid");
    let sentinel = invalid.n(1_i128 << 48);
    let mut raw = cursor_graph("reader-blob");
    let end = raw.skip_blob(input(1));
    let mut graphs = vec![
        invalid.finish(vec![sentinel]),
        raw.finish(vec![end]),
        string_graph(),
        sequence("reader-identity", &[]),
        sequence("reader-two-strings", &[Blob, Blob]),
        sequence("reader-strings", &[Repeat("reader-string")]),
        sequence("reader-string-strings", &[Blob, Repeat("reader-string")]),
        sequence("reader-byte", &[Fixed(1)]),
        sequence("reader-two", &[Fixed(2)]),
        sequence("reader-eight", &[Fixed(8)]),
        sequence("reader-sixteen", &[Fixed(16)]),
        sequence("reader-thirty-two", &[Fixed(32)]),
        sequence("reader-four", &[Fixed(4)]),
        sequence("reader-three", &[Fixed(3)]),
        sequence("reader-task", &[Blob, Fixed(16)]),
        sequence("reader-loop-operation", &[Blob, Blob, Fixed(8)]),
        sequence("reader-arm", &[Blob, Blob]),
        sequence("reader-match-operation", &[Repeat("reader-arm"), Blob]),
        sequence("reader-literal-text", &[Blob]),
    ];
    graphs.extend(dispatch(
        "reader-bool",
        &[(0, "reader-identity"), (1, "reader-identity")],
    ));
    graphs.extend(dispatch(
        "reader-float",
        &[(0, "reader-identity"), (1, "reader-eight")],
    ));
    graphs.extend(dispatch("reader-authority", &[(0, "reader-identity")]));
    graphs.extend(dispatch(
        "reader-effect",
        &(0..=10)
            .map(|tag| (tag, "reader-identity"))
            .collect::<Vec<_>>(),
    ));
    graphs.extend(dispatch(
        "reader-capability-class",
        &(0..=10)
            .map(|tag| (tag, "reader-identity"))
            .collect::<Vec<_>>(),
    ));
    graphs.extend(dispatch(
        "reader-literal",
        &[
            (0, "reader-bool"),
            (1, "reader-sixteen"),
            (2, "reader-literal-text"),
            (3, "reader-blob"),
        ],
    ));
    let operations: Vec<_> = (0..=74)
        .map(|tag| {
            let parser = match tag {
                0 => "reader-literal",
                4 => "reader-two-strings",
                5 => "reader-match-operation",
                6 => "reader-loop-operation",
                7..=9 | 15 | 38 | 39 | 47 | 55 | 63 | 65 => {
                    if tag == 38 {
                        "reader-string-strings"
                    } else {
                        "reader-string"
                    }
                }
                10..=12 => "reader-string-strings",
                13 | 14 => "reader-string",
                16 | 40 | 56..=59 => "reader-two-strings",
                29 => "reader-three",
                41 => "reader-string",
                54 | 62 | 64 => "reader-task",
                61 => "reader-byte",
                _ => "reader-identity",
            };
            (tag, parser)
        })
        .collect();
    graphs.extend(dispatch("reader-operation", &operations));
    // Types are prefix trees. The loop consumes one tag and updates the number
    // of outstanding child types, so nested types do not need recursive calls.
    graphs.extend(type_graphs(false));
    graphs.extend(type_graphs(true));
    graphs.push(sequence(
        "reader-port",
        &[Fixed(2), Blob, Call("reader-type")],
    ));
    graphs.push(sequence(
        "reader-port-layout",
        &[Fixed(2), Blob, Call("reader-type-layout")],
    ));
    graphs.push(sequence(
        "reader-capability",
        &[Call("reader-capability-class"), Blob, Blob, Blob],
    ));
    graphs.push(sequence(
        "reader-node",
        &[
            Fixed(4),
            Call("reader-operation"),
            Repeat("reader-port"),
            Repeat("reader-port"),
            Repeat("reader-effect"),
            Repeat("reader-capability"),
        ],
    ));
    graphs.push(sequence(
        "reader-node-checked",
        &[
            Fixed(4),
            Call("reader-operation-checked"),
            Repeat("reader-port"),
            Repeat("reader-port"),
            Repeat("reader-effect"),
            Repeat("reader-capability"),
        ],
    ));
    graphs.extend(dispatch(
        "reader-source",
        &[(0, "reader-two"), (1, "reader-six")],
    ));
    graphs.push(sequence(
        "reader-node-layout",
        &[
            Fixed(4),
            Call("reader-operation"),
            Repeat("reader-port-layout"),
            Repeat("reader-port-layout"),
            Repeat("reader-effect"),
            Repeat("reader-capability"),
        ],
    ));
    graphs.push(sequence(
        "reader-node-syntax",
        &[
            Fixed(4),
            Call("reader-operation-checked"),
            Repeat("reader-port-layout"),
            Repeat("reader-port-layout"),
            Repeat("reader-effect"),
            Repeat("reader-capability"),
        ],
    ));
    graphs.extend(dispatch(
        "reader-target",
        &[(0, "reader-six"), (1, "reader-two")],
    ));
    graphs.push(sequence("reader-six", &[Fixed(6)]));
    graphs.push(sequence(
        "reader-edge",
        &[Call("reader-source"), Call("reader-target")],
    ));
    graphs.push(sequence(
        "reader-graph-content",
        &[
            Blob,
            Call("reader-authority"),
            Repeat("reader-port"),
            Repeat("reader-port"),
            Repeat("reader-node-checked"),
            Repeat("reader-edge"),
        ],
    ));
    graphs.push(sequence(
        "reader-schema",
        &[Blob, Fixed(4), Repeat("reader-schema-field")],
    ));
    graphs.push(sequence(
        "reader-graph-content-syntax",
        &[
            Blob,
            Call("reader-authority"),
            Repeat("reader-port-layout"),
            Repeat("reader-port-layout"),
            Repeat("reader-node-syntax"),
            Repeat("reader-edge"),
        ],
    ));
    graphs.push(sequence(
        "reader-schema-field",
        &[
            Fixed(4),
            Blob,
            Call("reader-framed-type"),
            Call("reader-bool"),
        ],
    ));
    graphs.push(framed_type_graph());
    for name in [
        "reader-port",
        "reader-port-layout",
        "reader-byte",
        "reader-effect",
        "reader-capability",
        "reader-node",
        "reader-node-checked",
        "reader-node-layout",
        "reader-node-syntax",
        "reader-edge",
        "reader-string",
        "reader-arm",
        "reader-schema",
        "reader-schema-field",
    ] {
        graphs.extend(list_graphs(name));
    }
    let mut graph = cursor_graph("reader-graph-header");
    let magic = graph.u32(input(1));
    let expected = graph.n(0x00473047);
    let magic_ok = graph.compare(Operation::Eq, magic, expected);
    let version_at = graph.advance(input(1), 4);
    let version = graph.u32(version_at);
    let expected = graph.n(0x000a0000);
    let version_ok = graph.compare(Operation::Le, version.clone(), expected);
    let lowest = graph.n(0x00010000);
    let version_low = graph.compare(Operation::Ge, version.clone(), lowest);
    let multiple = graph.n(65536);
    let rem = graph.op(
        Operation::Rem,
        vec![(version, int()), (multiple, int())],
        int(),
    );
    let zero = graph.n(0);
    let major_ok = graph.compare(Operation::Eq, rem, zero);
    let valid = graph.and(magic_ok, version_ok);
    let valid = graph.and(valid, version_low);
    let valid = graph.and(valid, major_ok);
    let content = graph.advance(input(1), 8);
    let out = graph.op(
        Operation::Select {
            when_true: "reader-graph-content".into(),
            when_false: "reader-invalid".into(),
        },
        vec![
            (valid, SemanticType::Bool),
            (input(0), SemanticType::Bytes),
            (content, int()),
        ],
        int(),
    );
    let node = graph.b.graph.nodes.last_mut().unwrap();
    node.inputs[0].name = "selector".into();
    node.inputs[1].name = "p0".into();
    node.inputs[2].name = "p1".into();
    let checked_header = graph.finish(vec![out]);
    let mut layout_header = checked_header.clone();
    layout_header.name = "reader-graph-syntax".into();
    for node in &mut layout_header.nodes {
        if let Operation::Select { when_true, .. } = &mut node.operation
            && when_true == "reader-graph-content"
        {
            *when_true = "reader-graph-content-syntax".into();
        }
    }
    let mut semantic = cursor_graph("reader-graph");
    let end = semantic.call("reader-graph-header", input(1));
    let structure = semantic.op(
        Operation::Subgraph("validator-graph-edges".into()),
        vec![(input(0), SemanticType::Bytes), (input(1), int())],
        SemanticType::Bool,
    );
    let end = semantic.guard(structure, end);
    graphs.extend([checked_header, layout_header, semantic.finish(vec![end])]);
    graphs
}

fn type_graphs(layout: bool) -> Vec<Graph> {
    let prefix = if layout {
        "reader-type-layout"
    } else {
        "reader-type"
    };
    let state = vec![SemanticType::Bytes, int(), int()];
    let mut graphs = if layout {
        Vec::new()
    } else {
        signed_integer_graphs()
    };
    if !layout {
        graphs.push(integer_bounds_graph());
    }
    let payloads = [
        0, 32, 0, 0, 8, 0, 4, 0, 0, 8, 0, 8, -1, -1, 0, 0, -1, 0, 0, 0, 0, 0, 0, 0, 0,
    ];
    // Keep Bool and Integer first, then the text/byte/collection types used
    // throughout compiler definitions. Wire tags and payload handlers stay
    // unchanged; only the pure G0 dispatch chain is reordered.
    let dispatch_order = [
        0, 1, 7, 8, 10, 14, 15, 9, 11, 12, 13, 2, 3, 4, 5, 6, 16, 17, 18, 19, 20, 21, 22, 23, 24,
    ];
    for (position, tag) in dispatch_order.into_iter().enumerate() {
        let payload = payloads[tag];
        let name = format!("{prefix}-tag-{tag}");
        let mut g = G::new(&name, state.clone(), state.clone());
        let at = g.advance(input(1), 1);
        let at = if tag == 5 {
            g.call("reader-float", at)
        } else if payload < 0 {
            g.call("reader-string", at)
        } else {
            g.advance(at, payload)
        };
        let at = if tag == 1 && !layout {
            let minimum_at = g.advance(input(1), 1);
            let valid = g.op(
                Operation::Subgraph("reader-i128-bounds".into()),
                vec![(input(0), SemanticType::Bytes), (minimum_at, int())],
                SemanticType::Bool,
            );
            g.guard(valid, at)
        } else if matches!(tag, 4 | 6) && !layout {
            let precision_at = g.advance(input(1), 1);
            let precision = g.u32(precision_at);
            let lowest = g.n(if tag == 4 { 1 } else { 2 });
            let valid = g.compare(Operation::Ge, precision, lowest);
            g.guard(valid, at)
        } else {
            at
        };
        let pending = if tag == 15 {
            g.advance(input(2), 1)
        } else if matches!(tag,9..=11|14|17..=24) {
            input(2)
        } else {
            let one = g.n(1);
            g.arithmetic(Operation::Sub, input(2), one)
        };
        graphs.push(g.finish(vec![input(0), at, pending]));
        let dispatch_name = if tag == 0 {
            format!("{prefix}-step")
        } else {
            format!("{prefix}-step-{tag}")
        };
        let mut dispatch = G::new(&dispatch_name, state.clone(), state.clone());
        let byte = dispatch.byte(input(1));
        let expected = dispatch.n(tag as i128);
        let test = dispatch.compare(Operation::Eq, byte, expected);
        let fallback = if position + 1 == dispatch_order.len() {
            format!("{prefix}-invalid")
        } else {
            format!("{prefix}-step-{}", dispatch_order[position + 1])
        };
        let id = dispatch.b.graph.nodes.len() as u32 + 1;
        dispatch.op(
            Operation::Select {
                when_true: name,
                when_false: fallback,
            },
            vec![
                (test, SemanticType::Bool),
                (input(0), SemanticType::Bytes),
                (input(1), int()),
                (input(2), int()),
            ],
            SemanticType::Bytes,
        );
        let node = dispatch.b.graph.nodes.last_mut().unwrap();
        node.inputs[0].name = "selector".into();
        for i in 1..4 {
            node.inputs[i].name = format!("p{}", i - 1);
        }
        node.outputs = state
            .iter()
            .enumerate()
            .map(|(i, t)| port(i as u16, t.clone()))
            .collect();
        graphs.push(
            dispatch.finish(
                (0..3)
                    .map(|port| SourceEndpoint::NodeOutput { node: id, port })
                    .collect(),
            ),
        );
    }
    let mut invalid = G::new(&format!("{prefix}-invalid"), state.clone(), state.clone());
    let sentinel = invalid.n(1_i128 << 48);
    let zero = invalid.n(0);
    graphs.push(invalid.finish(vec![input(0), sentinel, zero]));
    let mut cond = G::new(
        &format!("{prefix}-condition"),
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let test = cond.compare(Operation::Gt, input(2), zero);
    graphs.push(cond.finish(vec![test]));
    let mut main = cursor_graph(prefix);
    let one = main.n(1);
    let id = main.b.graph.nodes.len() as u32 + 1;
    main.op(
        Operation::Loop {
            condition: format!("{prefix}-condition"),
            body: format!("{prefix}-step"),
            max_iterations: 131072,
        },
        vec![
            (input(0), SemanticType::Bytes),
            (input(1), int()),
            (one, int()),
        ],
        SemanticType::Bytes,
    );
    main.b.graph.nodes.last_mut().unwrap().outputs = state
        .into_iter()
        .enumerate()
        .map(|(i, t)| port(i as u16, t))
        .collect();
    graphs.push(main.finish(vec![SourceEndpoint::NodeOutput { node: id, port: 1 }]));
    graphs
}

fn full_integer() -> SemanticType {
    SemanticType::Integer(IntegerType {
        min: i128::MIN,
        max: i128::MAX,
    })
}

fn integer_bounds_graph() -> Graph {
    let mut graph = G::new(
        "reader-i128-bounds",
        vec![SemanticType::Bytes, int()],
        vec![SemanticType::Bool],
    );
    let length = graph.n(16);
    let maximum_at = graph.advance(input(1), 16);
    let minimum = graph.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (input(1), int()),
            (length.clone(), int()),
        ],
        SemanticType::Bytes,
    );
    let maximum = graph.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (maximum_at, int()),
            (length, int()),
        ],
        SemanticType::Bytes,
    );
    let minimum = graph.op(
        Operation::DecodeInteger128Le,
        vec![(minimum, SemanticType::Bytes)],
        full_integer(),
    );
    let maximum = graph.op(
        Operation::DecodeInteger128Le,
        vec![(maximum, SemanticType::Bytes)],
        full_integer(),
    );
    let valid = graph.compare(Operation::Le, minimum, maximum);
    graph.finish(vec![valid])
}

fn version_validation_graphs() -> Vec<Graph> {
    let mut graphs = Vec::new();
    let parameters = vec![SemanticType::Bytes, int()];
    for (index, maximum) in [29, 48, 57, 59, 61, 63, 65, 69, 73, 74]
        .into_iter()
        .enumerate()
    {
        let minor = index + 1;
        let mut constant = G::new(
            &format!("validator-version-maximum-{minor}"),
            parameters.clone(),
            vec![int()],
        );
        let value = constant.n(maximum);
        graphs.push(constant.finish(vec![value]));
        if minor == 1 {
            continue;
        }
        let mut dispatch = G::new(
            &format!("validator-version-limit-{minor}"),
            parameters.clone(),
            vec![int()],
        );
        let expected = dispatch.n((minor as i128) << 16);
        let test = dispatch.compare(Operation::Ge, input(1), expected);
        let fallback = if minor == 2 {
            "validator-version-maximum-1".into()
        } else {
            format!("validator-version-limit-{}", minor - 1)
        };
        let value = dispatch.op(
            Operation::Select {
                when_true: format!("validator-version-maximum-{minor}"),
                when_false: fallback,
            },
            vec![
                (test, SemanticType::Bool),
                (input(0), SemanticType::Bytes),
                (input(1), int()),
            ],
            int(),
        );
        let node = dispatch.b.graph.nodes.last_mut().unwrap();
        node.inputs[0].name = "selector".into();
        node.inputs[1].name = "p0".into();
        node.inputs[2].name = "p1".into();
        graphs.push(dispatch.finish(vec![value]));
    }
    let mut checked = cursor_graph("reader-operation-checked");
    let header = checked.n(4);
    let version = checked.u32(header);
    let maximum = checked.call("validator-version-limit-10", version);
    let tag = checked.byte(input(1));
    let valid = checked.compare(Operation::Le, tag, maximum);
    let end = checked.call("reader-operation", input(1));
    let end = checked.guard(valid, end);
    graphs.push(checked.finish(vec![end]));
    graphs
}

fn signed_integer_graphs() -> Vec<Graph> {
    let full = full_integer();
    let result = SemanticType::Result(Box::new(full.clone()), Box::new(SemanticType::Bool));
    let state = vec![SemanticType::Bytes, int(), int(), full.clone()];
    let mut condition = G::new(
        "reader-i128-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = condition.n(0);
    let test = condition.compare(Operation::Gt, input(2), zero);
    let mut body = G::new("reader-i128-body", state.clone(), state.clone());
    let one = body.n(1);
    let remaining = body.arithmetic(Operation::Sub, input(2), one);
    let offset = body.arithmetic(Operation::Add, input(1), remaining.clone());
    let byte = body.byte(offset);
    let factor = body.n(256);
    let multiplied = body.op(
        Operation::CheckedMul,
        vec![(input(3), full.clone()), (factor, full.clone())],
        result.clone(),
    );
    let zero = body.n(0);
    let multiplied = body.op(
        Operation::UnwrapOr,
        vec![(multiplied, result.clone()), (zero, full.clone())],
        full.clone(),
    );
    let added = body.op(
        Operation::CheckedAdd,
        vec![(multiplied, full.clone()), (byte, full.clone())],
        result.clone(),
    );
    let zero = body.n(0);
    let value = body.op(
        Operation::UnwrapOr,
        vec![(added, result), (zero, full.clone())],
        full.clone(),
    );
    let mut main = G::new(
        "reader-i128-word",
        vec![SemanticType::Bytes, int()],
        vec![full.clone()],
    );
    let length = main.n(16);
    let word = main.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (input(1), int()),
            (length, int()),
        ],
        SemanticType::Bytes,
    );
    let decoded = main.op(
        Operation::DecodeInteger128Le,
        vec![(word, SemanticType::Bytes)],
        full.clone(),
    );
    let mut bounded = G::new(
        "reader-i128",
        vec![SemanticType::Bytes, int()],
        vec![full.clone()],
    );
    let length = bounded.n(16);
    let word = bounded.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (input(1), int()),
            (length, int()),
        ],
        SemanticType::Bytes,
    );
    let zero = bounded.n(0);
    let decoded_word = bounded.op(
        Operation::Subgraph("reader-i128-word".into()),
        vec![(word, SemanticType::Bytes), (zero, int())],
        full,
    );
    vec![
        condition.finish(vec![test]),
        body.finish(vec![input(0), input(1), remaining, value]),
        main.finish(vec![decoded]),
        bounded.finish(vec![decoded_word]),
    ]
}

fn ast_graphs() -> Vec<Graph> {
    let descriptor = SemanticType::Slice(Box::new(int()));
    let collection = SemanticType::Slice(Box::new(descriptor.clone()));
    let state = vec![SemanticType::Bytes, int(), int(), collection.clone()];
    let mut condition = G::new(
        "reader-ast-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = condition.n(0);
    let test = condition.compare(Operation::Gt, input(2), zero);
    let mut body = G::new("reader-ast-body", state.clone(), state.clone());
    let id = body.u32(input(1));
    let operation = body.advance(input(1), 4);
    let inputs = body.call("reader-operation", operation.clone());
    let outputs = body.call("reader-list-reader-port-layout", inputs.clone());
    let effects = body.call("reader-list-reader-port-layout", outputs.clone());
    let capabilities = body.call("reader-list-reader-byte", effects.clone());
    let end = body.call("reader-list-reader-capability", capabilities.clone());
    let descriptor_value = body.op(
        Operation::MakeArray,
        vec![
            id,
            input(1),
            operation,
            inputs,
            outputs,
            effects,
            capabilities,
            end.clone(),
        ]
        .into_iter()
        .map(|e| (e, int()))
        .collect(),
        descriptor.clone(),
    );
    let singleton = body.op(
        Operation::MakeArray,
        vec![(descriptor_value, descriptor.clone())],
        collection.clone(),
    );
    let accumulated = body.op(
        Operation::ArrayConcat,
        vec![
            (input(3), collection.clone()),
            (singleton, collection.clone()),
        ],
        collection.clone(),
    );
    let one = body.n(1);
    let remaining = body.arithmetic(Operation::Sub, input(2), one);
    let mut main = G::new(
        "reader-graph-ast",
        vec![SemanticType::Bytes, int()],
        vec![collection.clone()],
    );
    let at = main.advance(input(1), 8);
    let at = main.skip_blob(at);
    let at = main.advance(at, 1);
    let at = main.call("reader-list-reader-port-layout", at);
    let at = main.call("reader-list-reader-port-layout", at);
    let count = main.u32(at.clone());
    let start = main.advance(at, 4);
    let empty = main.op(Operation::MakeArray, vec![], collection.clone());
    let loop_id = main.b.graph.nodes.len() as u32 + 1;
    main.op(
        Operation::Loop {
            condition: "reader-ast-condition".into(),
            body: "reader-ast-body".into(),
            max_iterations: 131072,
        },
        vec![
            (input(0), SemanticType::Bytes),
            (start, int()),
            (count, int()),
            (empty, collection),
        ],
        SemanticType::Bytes,
    );
    main.b.graph.nodes.last_mut().unwrap().outputs = state
        .into_iter()
        .enumerate()
        .map(|(i, t)| port(i as u16, t))
        .collect();
    vec![
        condition.finish(vec![test]),
        body.finish(vec![input(0), end, remaining, accumulated]),
        main.finish(vec![SourceEndpoint::NodeOutput {
            node: loop_id,
            port: 3,
        }]),
    ]
}

fn program_ast_graphs() -> Vec<Graph> {
    let descriptor = SemanticType::Slice(Box::new(int()));
    let collection = SemanticType::Slice(Box::new(descriptor.clone()));
    let state = vec![SemanticType::Bytes, int(), int(), collection.clone()];
    let mut condition = G::new(
        "reader-program-ast-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = condition.n(0);
    let remaining = condition.compare(Operation::Gt, input(2), zero);
    let mut layout = G::new(
        "reader-program-graph-layout",
        vec![SemanticType::Bytes],
        vec![descriptor.clone()],
    );
    let local_name = layout.n(8);
    let authority = layout.skip_blob(local_name.clone());
    let local_inputs = layout.advance(authority, 1);
    let local_outputs = layout.call("reader-list-reader-port-layout", local_inputs.clone());
    let local_nodes = layout.call("reader-list-reader-port-layout", local_outputs.clone());
    let local_edges = layout.call("reader-list-reader-node-layout", local_nodes.clone());
    let local_row = layout.op(
        Operation::MakeArray,
        vec![
            local_name,
            local_inputs,
            local_outputs,
            local_nodes,
            local_edges,
        ]
        .into_iter()
        .map(|e| (e, int()))
        .collect(),
        descriptor.clone(),
    );
    let mut body = G::new("reader-program-ast-body", state.clone(), state.clone());
    let size = body.u32(input(1));
    let start = body.advance(input(1), 4);
    let end = body.arithmetic(Operation::Add, start.clone(), size.clone());
    let bytes = body.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (start.clone(), int()),
            (size, int()),
        ],
        SemanticType::Bytes,
    );
    let local = body.op(
        Operation::Subgraph("reader-program-graph-layout".into()),
        vec![(bytes, SemanticType::Bytes)],
        descriptor.clone(),
    );
    let mut fields = vec![start.clone(), end.clone()];
    for index in 0..5 {
        let offset = body.field(local.clone(), index);
        fields.push(body.arithmetic(Operation::Add, start.clone(), offset));
    }
    let row = body.op(
        Operation::MakeArray,
        fields.into_iter().map(|e| (e, int())).collect(),
        descriptor.clone(),
    );
    let singleton = body.op(
        Operation::MakeArray,
        vec![(row, descriptor)],
        collection.clone(),
    );
    let rows = body.op(
        Operation::ArrayConcat,
        vec![
            (input(3), collection.clone()),
            (singleton, collection.clone()),
        ],
        collection.clone(),
    );
    let one = body.n(1);
    let count = body.arithmetic(Operation::Sub, input(2), one);
    let mut main = G::new(
        "reader-program-ast",
        vec![SemanticType::Bytes],
        vec![collection.clone()],
    );
    let eight = main.n(8);
    let count_at = main.skip_blob(eight);
    let count_value = main.u32(count_at.clone());
    let cursor = main.advance(count_at, 4);
    let empty = main.op(Operation::MakeArray, vec![], collection);
    let result = main.loop_node(
        "reader-program-ast",
        state,
        vec![input(0), cursor, count_value, empty],
    );
    vec![
        layout.finish(vec![local_row]),
        condition.finish(vec![remaining]),
        body.finish(vec![input(0), end, count, rows]),
        main.finish(vec![result[3].clone()]),
    ]
}

fn string_graph() -> Graph {
    let mut g = cursor_graph("reader-string");
    let length = g.u32(input(1));
    let start = g.advance(input(1), 4);
    let bytes = g.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (start.clone(), int()),
            (length.clone(), int()),
        ],
        SemanticType::Bytes,
    );
    let result_type =
        SemanticType::Result(Box::new(SemanticType::Text), Box::new(SemanticType::Bytes));
    let result = g.op(
        Operation::DecodeUtf8,
        vec![(bytes, SemanticType::Bytes)],
        result_type.clone(),
    );
    let empty = g.op(
        Operation::Const(Literal::Text(String::new())),
        vec![],
        SemanticType::Text,
    );
    let text = g.op(
        Operation::UnwrapOr,
        vec![(result, result_type), (empty, SemanticType::Text)],
        SemanticType::Text,
    );
    let encoded = g.op(
        Operation::EncodeUtf8,
        vec![(text, SemanticType::Text)],
        SemanticType::Bytes,
    );
    let encoded_length = g.op(
        Operation::Length,
        vec![(encoded, SemanticType::Bytes)],
        SemanticType::Integer(IntegerType {
            min: 0,
            max: u64::MAX as i128,
        }),
    );
    let valid = g.compare(Operation::Eq, encoded_length, length.clone());
    let end = g.arithmetic(Operation::Add, start, length);
    let end = g.guard(valid, end);
    g.finish(vec![end])
}
fn framed_type_graph() -> Graph {
    let mut g = cursor_graph("reader-framed-type");
    let length = g.u32(input(1));
    let start = g.advance(input(1), 4);
    let bytes = g.op(
        Operation::BytesSlice,
        vec![
            (input(0), SemanticType::Bytes),
            (start.clone(), int()),
            (length.clone(), int()),
        ],
        SemanticType::Bytes,
    );
    let zero = g.n(0);
    let actual = g.op(
        Operation::Subgraph("reader-type".into()),
        vec![(bytes, SemanticType::Bytes), (zero, int())],
        int(),
    );
    let valid = g.compare(Operation::Eq, actual, length.clone());
    let end = g.arithmetic(Operation::Add, start, length);
    let end = g.guard(valid, end);
    g.finish(vec![end])
}

fn scheduler_graphs() -> Vec<Graph> {
    let seq = SemanticType::Slice(Box::new(int()));
    let nodes = SemanticType::Slice(Box::new(seq.clone()));
    let mut graphs = Vec::new();
    for (name, ty) in [
        ("scheduler-pick-ids", seq.clone()),
        ("scheduler-pick-nodes", nodes.clone()),
    ] {
        for (suffix, p) in [("first", 0), ("second", 1)] {
            let g = G::new(
                &format!("{name}-{suffix}"),
                vec![ty.clone(), ty.clone()],
                vec![ty.clone()],
            );
            graphs.push(g.finish(vec![input(p)]));
        }
    }
    let state = vec![seq.clone(), int(), int(), SemanticType::Bool];
    let mut cond = G::new(
        "scheduler-contains-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(0));
    let before = cond.compare(Operation::Lt, input(2), len);
    let notfound = cond.not(input(3));
    let test = cond.and(before, notfound);
    let mut body = G::new("scheduler-contains-body", state.clone(), state.clone());
    let value = body.index(input(0), input(2));
    let found = body.compare(Operation::Eq, value, input(1));
    let next = body.advance(input(2), 1);
    let mut main = G::new(
        "scheduler-contains",
        vec![seq.clone(), int()],
        vec![SemanticType::Bool],
    );
    let zero = main.n(0);
    let no = main.bool(false);
    let out = main.loop_node(
        "scheduler-contains",
        state,
        vec![input(0), input(1), zero, no],
    );
    graphs.extend([
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), next, found]),
        main.finish(vec![out[3].clone()]),
    ]);
    let state = vec![nodes.clone(), int(), seq.clone(), int(), SemanticType::Bool];
    let mut cond = G::new(
        "scheduler-ready-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let len = cond.length(input(0));
    let before = cond.compare(Operation::Lt, input(3), len);
    let test = cond.and(before, input(4));
    let mut body = G::new("scheduler-ready-body", state.clone(), state.clone());
    let edge = body.index(input(0), input(3));
    let source_kind = body.field(edge.clone(), 0);
    let source_node = body.field(edge.clone(), 1);
    let target_kind = body.field(edge.clone(), 3);
    let target_node = body.field(edge, 4);
    let one = body.n(1);
    let source_is_node = body.compare(Operation::Eq, source_kind, one);
    let zero = body.n(0);
    let target_is_node = body.compare(Operation::Eq, target_kind, zero);
    let target_is_this = body.compare(Operation::Eq, target_node, input(1));
    let dependency = body.and(source_is_node, target_is_node);
    let dependency = body.and(dependency, target_is_this);
    let contained = body.op(
        Operation::Subgraph("scheduler-contains".into()),
        vec![(input(2), seq.clone()), (source_node, int())],
        SemanticType::Bool,
    );
    let irrelevant = body.not(dependency);
    let satisfied = body.logic(Operation::Or, irrelevant, contained);
    let next = body.advance(input(3), 1);
    let mut main = G::new(
        "scheduler-ready",
        vec![nodes.clone(), int(), seq.clone()],
        vec![SemanticType::Bool],
    );
    let zero = main.n(0);
    let yes = main.bool(true);
    let out = main.loop_node(
        "scheduler-ready",
        state,
        vec![input(0), input(1), input(2), zero, yes],
    );
    graphs.extend([
        cond.finish(vec![test]),
        body.finish(vec![input(0), input(1), input(2), next, satisfied]),
        main.finish(vec![out[4].clone()]),
    ]);
    let state = vec![
        nodes.clone(),
        nodes.clone(),
        seq.clone(),
        nodes.clone(),
        int(),
        SemanticType::Bool,
        SemanticType::Bool,
    ];
    let mut cond = G::new(
        "scheduler-order-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let total = cond.length(input(0));
    let count = cond.length(input(3));
    let pending = cond.compare(Operation::Lt, count, total);
    let test = cond.and(pending, input(6));
    let mut body = G::new("scheduler-order-body", state.clone(), state.clone());
    let candidate = body.index(input(0), input(4));
    let id = body.field(candidate.clone(), 0);
    let contained = body.op(
        Operation::Subgraph("scheduler-contains".into()),
        vec![(input(2), seq.clone()), (id.clone(), int())],
        SemanticType::Bool,
    );
    let unvisited = body.not(contained);
    let ready = body.op(
        Operation::Subgraph("scheduler-ready".into()),
        vec![
            (input(1), nodes.clone()),
            (id.clone(), int()),
            (input(2), seq.clone()),
        ],
        SemanticType::Bool,
    );
    let eligible = body.and(unvisited, ready);
    let single = body.op(Operation::MakeArray, vec![(id, int())], seq.clone());
    let appended = body.op(
        Operation::ArrayConcat,
        vec![(input(2), seq.clone()), (single, seq.clone())],
        seq.clone(),
    );
    let ids = body.pick("scheduler-pick-ids", eligible.clone(), input(2), appended);
    let single = body.op(
        Operation::MakeArray,
        vec![(candidate, seq.clone())],
        nodes.clone(),
    );
    let appended = body.op(
        Operation::ArrayConcat,
        vec![(input(3), nodes.clone()), (single, nodes.clone())],
        nodes.clone(),
    );
    let ordered = body.pick("scheduler-pick-nodes", eligible.clone(), input(3), appended);
    let progress = body.logic(Operation::Or, input(5), eligible);
    let next = body.advance(input(4), 1);
    let total = body.length(input(0));
    let end = body.compare(Operation::Ge, next.clone(), total.clone());
    let continuing = body.not(end);
    let valid = body.logic(Operation::Or, continuing.clone(), progress.clone());
    let new_progress = body.and(continuing, progress);
    let divisor = body.op(
        Operation::ConvertChecked,
        vec![(
            total,
            SemanticType::Integer(IntegerType {
                min: 0,
                max: u64::MAX as i128,
            }),
        )],
        int(),
    );
    let next = body.op(Operation::Rem, vec![(next, int()), (divisor, int())], int());
    let mut main = G::new(
        "scheduler-order",
        vec![nodes.clone(), nodes.clone()],
        vec![nodes.clone()],
    );
    let empty_ids = main.op(Operation::MakeArray, vec![], seq);
    let empty_nodes = main.op(Operation::MakeArray, vec![], nodes.clone());
    let zero = main.n(0);
    let no = main.bool(false);
    let yes = main.bool(true);
    let out = main.loop_node(
        "scheduler-order",
        state,
        vec![input(0), input(1), empty_ids, empty_nodes, zero, no, yes],
    );
    let total = main.length(input(0));
    let count = main.length(out[3].clone());
    let complete = main.compare(Operation::Eq, count, total);
    let empty = main.op(Operation::MakeArray, vec![], nodes);
    let result = main.pick("scheduler-pick-nodes", complete, empty, out[3].clone());
    graphs.extend([
        cond.finish(vec![test]),
        body.finish(vec![
            input(0),
            input(1),
            ids,
            ordered,
            next,
            new_progress,
            valid,
        ]),
        main.finish(vec![result]),
    ]);
    graphs
}

fn edge_ast_graphs() -> Vec<Graph> {
    let descriptor = SemanticType::Slice(Box::new(int()));
    let collection = SemanticType::Slice(Box::new(descriptor.clone()));
    let mut graphs = Vec::new();
    for (suffix, p) in [("first", 0), ("second", 1)] {
        let g = G::new(
            &format!("reader-pick-offset-{suffix}"),
            vec![int(), int()],
            vec![int()],
        );
        graphs.push(g.finish(vec![input(p)]));
    }
    for (name, node_kind) in [("reader-source-ast", 1), ("reader-target-ast", 0)] {
        let mut g = G::new(
            name,
            vec![SemanticType::Bytes, int()],
            vec![descriptor.clone()],
        );
        let kind = g.byte(input(1));
        let expected = g.n(node_kind);
        let has_node = g.compare(Operation::Eq, kind.clone(), expected);
        let id_at = g.advance(input(1), 1);
        let id = g.u32(id_at.clone());
        let zero = g.n(0);
        let id = g.pick("reader-pick-offset", has_node.clone(), zero, id);
        let node_port = g.advance(input(1), 5);
        let port_at = g.pick("reader-pick-offset", has_node.clone(), id_at, node_port);
        let port = g.u32(port_at);
        let modulus = g.n(65536);
        let port = g.op(Operation::Rem, vec![(port, int()), (modulus, int())], int());
        let graph_end = g.advance(input(1), 3);
        let node_end = g.advance(input(1), 7);
        let end = g.pick("reader-pick-offset", has_node, graph_end, node_end);
        let out = g.op(
            Operation::MakeArray,
            vec![kind, id, port, end]
                .into_iter()
                .map(|e| (e, int()))
                .collect(),
            descriptor.clone(),
        );
        graphs.push(g.finish(vec![out]));
    }
    let state = vec![SemanticType::Bytes, int(), int(), collection.clone()];
    let mut cond = G::new(
        "reader-edge-ast-condition",
        state.clone(),
        vec![SemanticType::Bool],
    );
    let zero = cond.n(0);
    let test = cond.compare(Operation::Gt, input(2), zero);
    let mut body = G::new("reader-edge-ast-body", state.clone(), state.clone());
    let source = body.op(
        Operation::Subgraph("reader-source-ast".into()),
        vec![(input(0), SemanticType::Bytes), (input(1), int())],
        descriptor.clone(),
    );
    let source_end = body.field(source.clone(), 3);
    let target = body.op(
        Operation::Subgraph("reader-target-ast".into()),
        vec![(input(0), SemanticType::Bytes), (source_end, int())],
        descriptor.clone(),
    );
    let end = body.field(target.clone(), 3);
    let mut fields = Vec::new();
    for value in [source, target] {
        for i in 0..3 {
            fields.push((body.field(value.clone(), i), int()));
        }
    }
    let edge = body.op(Operation::MakeArray, fields, descriptor.clone());
    let singleton = body.op(
        Operation::MakeArray,
        vec![(edge, descriptor)],
        collection.clone(),
    );
    let accumulated = body.op(
        Operation::ArrayConcat,
        vec![
            (input(3), collection.clone()),
            (singleton, collection.clone()),
        ],
        collection.clone(),
    );
    let one = body.n(1);
    let remaining = body.arithmetic(Operation::Sub, input(2), one);
    let mut main = G::new(
        "reader-graph-edges",
        vec![SemanticType::Bytes, int()],
        vec![collection.clone()],
    );
    let at = main.advance(input(1), 8);
    let at = main.call("reader-string", at);
    let at = main.advance(at, 1);
    let at = main.call("reader-list-reader-port-layout", at);
    let at = main.call("reader-list-reader-port-layout", at);
    let at = main.call("reader-list-reader-node-layout", at);
    let count = main.u32(at.clone());
    let at = main.advance(at, 4);
    let empty = main.op(Operation::MakeArray, vec![], collection);
    let out = main.loop_node("reader-edge-ast", state, vec![input(0), at, count, empty]);
    graphs.extend([
        cond.finish(vec![test]),
        body.finish(vec![input(0), end, remaining, accumulated]),
        main.finish(vec![out[3].clone()]),
    ]);
    graphs
}
