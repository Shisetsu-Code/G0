use g0::{
    bootstrap_compiler::{compiler_document, compiler_limits},
    execution::Executor,
    gir::{IntegerType, SemanticType},
    value::Value,
};
fn int(min: i128, max: i128) -> SemanticType {
    SemanticType::Integer(IntegerType { min, max })
}
fn row(ns: &[usize]) -> Value {
    Value::Array(
        ns.iter()
            .map(|n| Value::Integer(*n as i128))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn run(name: &str, args: Vec<Value>) -> Vec<Value> {
    let p = compiler_document().validated_contract().unwrap();
    Executor::new(&p, compiler_limits())
        .unwrap()
        .run_graph(name, args)
        .unwrap()
}
fn string(source: &mut Vec<u8>, s: &str) -> usize {
    let at = source.len();
    source.extend_from_slice(&(s.len() as u32).to_le_bytes());
    source.extend_from_slice(s.as_bytes());
    at
}
fn ports(source: &mut Vec<u8>, ps: &[(u16, &str, SemanticType)]) -> usize {
    let at = source.len();
    source.extend_from_slice(&(ps.len() as u32).to_le_bytes());
    for (id, name, ty) in ps {
        source.extend_from_slice(&id.to_le_bytes());
        string(source, name);
        source.extend(g0::graph_binary::encode_semantic_type(ty).unwrap());
    }
    at
}
struct Fixture {
    source: Vec<u8>,
    graphs: Vec<Value>,
}
impl Fixture {
    fn new() -> Self {
        Self {
            source: vec![],
            graphs: vec![],
        }
    }
    fn graph(
        &mut self,
        name: &str,
        inputs: &[(u16, &str, SemanticType)],
        outputs: &[(u16, &str, SemanticType)],
    ) {
        let name = string(&mut self.source, name);
        let inputs = ports(&mut self.source, inputs);
        let outputs = ports(&mut self.source, outputs);
        self.graphs.push(row(&[0, 0, name, inputs, outputs, 0, 0]));
    }
    fn check(
        mut self,
        tag: u8,
        names: &[&str],
        bound: Option<u64>,
        inputs: &[(u16, &str, SemanticType)],
        outputs: &[(u16, &str, SemanticType)],
    ) -> bool {
        let op = self.source.len();
        self.source.push(tag);
        for name in names {
            string(&mut self.source, name);
        }
        if let Some(bound) = bound {
            self.source.extend_from_slice(&bound.to_le_bytes());
        }
        let inputs = ports(&mut self.source, inputs);
        let outputs = ports(&mut self.source, outputs);
        let effects = self.source.len();
        self.source.extend_from_slice(&0u32.to_le_bytes());
        let caps = self.source.len();
        self.source.extend_from_slice(&0u32.to_le_bytes());
        let end = self.source.len();
        let node = row(&[1, 0, op, inputs, outputs, effects, caps, end]);
        let result = run(
            "validator-node-control",
            vec![
                Value::Bytes(self.source.into()),
                Value::Array(self.graphs.into()),
                node,
            ],
        );
        let [Value::Bool(ok)] = result.as_slice() else {
            panic!()
        };
        *ok
    }
}
#[test]
fn controlcheck_subgraph_requires_exact_types_but_ignores_port_names_and_ids() {
    let make = || {
        let mut f = Fixture::new();
        f.graph(
            "worker",
            &[(2, "callee", int(0, 100))],
            &[(99, "answer", int(0, 100))],
        );
        f
    };
    assert!(make().check(
        7,
        &["worker"],
        None,
        &[(42, "caller", int(0, 100))],
        &[(7, "different", int(0, 100))]
    ));
    assert!(!make().check(
        7,
        &["worker"],
        None,
        &[(42, "caller", int(0, 99))],
        &[(7, "different", int(0, 100))]
    ));
    assert!(!make().check(
        7,
        &["missing"],
        None,
        &[(42, "caller", int(0, 100))],
        &[(7, "different", int(0, 100))]
    ));
}
#[test]
fn controlcheck_select_requires_bool_and_exact_named_branch_interfaces() {
    let make = || {
        let mut f = Fixture::new();
        for name in ["yes", "no"] {
            f.graph(
                name,
                &[(2, "payload", SemanticType::Text)],
                &[(99, "answer", int(0, 100))],
            );
        }
        f
    };
    let inputs = [
        (5, "condition", SemanticType::Bool),
        (20, "payload", SemanticType::Text),
    ];
    let output = [(7, "answer", int(0, 100))];
    assert!(make().check(4, &["yes", "no"], None, &inputs, &output));
    assert!(!make().check(
        4,
        &["yes", "no"],
        None,
        &[
            (5, "condition", int(0, 1)),
            (20, "payload", SemanticType::Text)
        ],
        &output
    ));
    assert!(!make().check(
        4,
        &["yes", "no"],
        None,
        &[
            (5, "condition", SemanticType::Bool),
            (20, "wrong", SemanticType::Text)
        ],
        &output
    ));
    assert!(!make().check(
        4,
        &["yes", "no"],
        None,
        &inputs,
        &[(7, "answer", int(0, 101))]
    ));
}

#[test]
fn controlcheck_loop_proves_condition_body_and_parallel_state_contracts() {
    let state = [
        (7, "flag", SemanticType::Bool),
        (99, "value", int(-100, 100)),
    ];
    let make = || {
        let mut f = Fixture::new();
        f.graph("condition", &state, &[(4, "continue", SemanticType::Bool)]);
        f.graph("body", &state, &state);
        f
    };
    assert!(make().check(6, &["condition", "body"], Some(u64::MAX), &state, &state));
    assert!(!make().check(6, &["condition", "body"], Some(0), &state, &state));
    assert!(!make().check(
        6,
        &["condition", "body"],
        Some(4),
        &state,
        &[
            (7, "flag", SemanticType::Bool),
            (99, "value", int(-99, 100))
        ]
    ));
    let mut f = Fixture::new();
    f.graph("condition", &state, &[(4, "continue", int(0, 1))]);
    f.graph("body", &state, &state);
    assert!(!f.check(6, &["condition", "body"], Some(4), &state, &state));
    let mut f = Fixture::new();
    f.graph("condition", &state, &[(4, "continue", SemanticType::Bool)]);
    f.graph(
        "body",
        &state,
        &[
            (7, "wrong", SemanticType::Bool),
            (99, "value", int(-100, 100)),
        ],
    );
    assert!(!f.check(6, &["condition", "body"], Some(4), &state, &state));
}
#[test]
fn controlcheck_map_requires_exact_element_contract_including_bytes_u8() {
    let make = |ty: SemanticType| {
        let mut f = Fixture::new();
        f.graph(
            "body",
            &[(7, "element", ty)],
            &[(99, "mapped", SemanticType::Text)],
        );
        f
    };
    let output = [(
        4,
        "output",
        SemanticType::Slice(Box::new(SemanticType::Text)),
    )];
    assert!(make(int(0, 255)).check(
        47,
        &["body"],
        None,
        &[(5, "input", SemanticType::Bytes)],
        &output
    ));
    assert!(!make(int(0, 256)).check(
        47,
        &["body"],
        None,
        &[(5, "input", SemanticType::Bytes)],
        &output
    ));
    assert!(make(int(0, 100)).check(
        47,
        &["body"],
        None,
        &[(5, "input", SemanticType::Array(Box::new(int(0, 100)), 3))],
        &output
    ));
    assert!(!make(int(0, 101)).check(
        47,
        &["body"],
        None,
        &[(5, "input", SemanticType::Slice(Box::new(int(0, 100))))],
        &output
    ));
    assert!(!make(int(0, 100)).check(
        47,
        &["body"],
        None,
        &[(5, "input", SemanticType::Slice(Box::new(int(0, 100))))],
        &[(
            4,
            "output",
            SemanticType::Array(Box::new(SemanticType::Text), 3)
        )]
    ));
    assert!(!make(int(0, 100)).check(
        47,
        &["body"],
        None,
        &[(5, "input", SemanticType::Bool)],
        &output
    ));
    assert!(!make(int(0, 100)).check(
        47,
        &["body"],
        None,
        &[(5, "input", SemanticType::Slice(Box::new(int(0, 100))))],
        &[(4, "output", SemanticType::Bool)]
    ));
}

#[test]
fn controlcheck_primitive_delegates_and_match_fails_closed() {
    assert!(Fixture::new().check(0, &[], None, &[], &[]));
    assert!(!Fixture::new().check(5, &[], None, &[], &[]));
}

#[test]
fn controlcheck_name_lookup_is_exact_for_unicode_and_missing_names() {
    let mut f = Fixture::new();
    f.graph("β", &[], &[]);
    f.graph("βx", &[], &[]);
    for (name, expected) in [("β", 0), ("βx", 1), ("βy", 2)] {
        let at = string(&mut f.source, name);
        assert_eq!(
            run(
                "validator-graph-name-index",
                vec![
                    Value::Bytes(f.source.clone().into()),
                    Value::Array(f.graphs.clone().into()),
                    Value::Integer(at as i128),
                ]
            ),
            vec![Value::Integer(expected)]
        );
    }
}
