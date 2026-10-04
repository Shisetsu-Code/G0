use g0::{
    bootstrap_compiler::{compiler_document, compiler_limits},
    execution::Executor,
    gir::{IntegerType, SemanticType},
    value::Value,
};
fn int(min: i128, max: i128) -> SemanticType {
    SemanticType::Integer(IntegerType { min, max })
}
fn row(values: &[usize]) -> Value {
    Value::Array(
        values
            .iter()
            .map(|n| Value::Integer(*n as i128))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn string(bytes: &mut Vec<u8>, value: &str) -> usize {
    let at = bytes.len();
    bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
    bytes.extend_from_slice(value.as_bytes());
    at
}
fn ports(bytes: &mut Vec<u8>, ps: &[(&str, SemanticType)]) -> usize {
    let at = bytes.len();
    bytes.extend_from_slice(&(ps.len() as u32).to_le_bytes());
    for (i, (name, ty)) in ps.iter().enumerate() {
        bytes.extend_from_slice(&(i as u16).to_le_bytes());
        string(bytes, name);
        bytes.extend(g0::graph_binary::encode_semantic_type(ty).unwrap());
    }
    at
}
struct Fixture {
    bytes: Vec<u8>,
    schemas: Vec<Value>,
    graphs: Vec<Value>,
}
impl Fixture {
    fn new() -> Self {
        let mut f = Self {
            bytes: vec![],
            schemas: vec![],
            graphs: vec![],
        };
        let name = string(&mut f.bytes, "Shape");
        let version = f.bytes.len();
        f.bytes.extend_from_slice(&1u32.to_le_bytes());
        let fields = f.bytes.len();
        f.bytes.extend_from_slice(&2u32.to_le_bytes());
        for (tag, name, ty, optional) in [
            (1u32, "name", SemanticType::Text, false),
            (2, "size", int(0, 100), true),
        ] {
            f.bytes.extend_from_slice(&tag.to_le_bytes());
            string(&mut f.bytes, name);
            let ty = g0::graph_binary::encode_semantic_type(&ty).unwrap();
            f.bytes.extend_from_slice(&(ty.len() as u32).to_le_bytes());
            f.bytes.extend(ty);
            f.bytes.push(u8::from(optional));
        }
        f.schemas
            .push(row(&[name, f.bytes.len(), name, version, fields, 0, 0]));
        f
    }
    fn branch(
        &mut self,
        name: &str,
        inputs: &[(&str, SemanticType)],
        outputs: &[(&str, SemanticType)],
    ) {
        let name = string(&mut self.bytes, name);
        let ins = ports(&mut self.bytes, inputs);
        let outs = ports(&mut self.bytes, outputs);
        self.graphs.push(row(&[0, 0, name, ins, outs, 0, 0]));
    }
    fn check(
        mut self,
        operation: Vec<u8>,
        inputs: &[(&str, SemanticType)],
        outputs: &[(&str, SemanticType)],
    ) -> bool {
        let op = self.bytes.len();
        self.bytes.extend(operation);
        let ins = ports(&mut self.bytes, inputs);
        let outs = ports(&mut self.bytes, outputs);
        let effects = self.bytes.len();
        self.bytes.extend_from_slice(&0u32.to_le_bytes());
        let caps = self.bytes.len();
        self.bytes.extend_from_slice(&0u32.to_le_bytes());
        let node = row(&[1, 0, op, ins, outs, effects, caps, self.bytes.len()]);
        let document = compiler_document();
        let contract = document.validated_contract().unwrap();
        let args = vec![
            Value::Bytes(self.bytes.into()),
            Value::Array(self.schemas.into()),
            Value::Array(self.graphs.into()),
            node,
        ];
        let result = Executor::new(&contract, compiler_limits())
            .unwrap()
            .run_graph("validator-node-schema", args.clone())
            .unwrap();
        assert_eq!(
            Executor::new(&contract, compiler_limits())
                .unwrap()
                .run_graph("validator-node-schema-fast", args)
                .unwrap(),
            result
        );
        let [Value::Bool(ok)] = result.as_slice() else {
            panic!()
        };
        *ok
    }
}
fn operation(tag: u8, names: &[&str]) -> Vec<u8> {
    let mut b = vec![tag];
    for name in names {
        string(&mut b, name);
    }
    b
}
fn record(fields: &[&str]) -> Vec<u8> {
    let mut b = operation(38, &["Shape"]);
    b.extend_from_slice(&(fields.len() as u32).to_le_bytes());
    for name in fields {
        string(&mut b, name);
    }
    b
}
#[test]
fn schema_record_checks_required_fields_names_and_assignability() {
    let out = [("value", SemanticType::Record("Shape".into()))];
    assert!(Fixture::new().check(record(&["name"]), &[("name", SemanticType::Text)], &out));
    assert!(Fixture::new().check(
        record(&["name", "size"]),
        &[("name", SemanticType::Text), ("size", int(2, 3))],
        &out
    ));
    assert!(!Fixture::new().check(record(&["size"]), &[("size", int(2, 3))], &out));
    assert!(!Fixture::new().check(
        record(&["name", "size"]),
        &[("name", SemanticType::Text), ("size", int(0, 101))],
        &out
    ));
    assert!(!Fixture::new().check(record(&["gone"]), &[("gone", SemanticType::Text)], &out));
    let mut missing_schema = Fixture::new();
    missing_schema.schemas.clear();
    assert!(!missing_schema.check(record(&["name"]), &[("name", SemanticType::Text)], &out));
    assert!(!Fixture::new().check(
        record(&["name", "name"]),
        &[
            ("first", SemanticType::Text),
            ("second", SemanticType::Text)
        ],
        &out
    ));
}
#[test]
fn schema_field_applies_optional_wrapper_and_covariant_integer_bounds() {
    let input = [("record", SemanticType::Record("Shape".into()))];
    assert!(Fixture::new().check(
        operation(39, &["name"]),
        &input,
        &[("value", SemanticType::Text)]
    ));
    assert!(Fixture::new().check(
        operation(39, &["size"]),
        &input,
        &[("value", SemanticType::Option(Box::new(int(-1, 101))))]
    ));
    assert!(!Fixture::new().check(operation(39, &["size"]), &input, &[("value", int(0, 100))]));
    assert!(!Fixture::new().check(
        operation(39, &["size"]),
        &input,
        &[("value", SemanticType::Option(Box::new(int(0, 99))))]
    ));
    assert!(!Fixture::new().check(
        operation(39, &["gone"]),
        &input,
        &[("value", SemanticType::Text)]
    ));
}
#[test]
fn schema_variant_constructor_and_payload_check_tag_types() {
    let variant = SemanticType::Variant("Shape".into());
    assert!(Fixture::new().check(
        operation(40, &["Shape", "size"]),
        &[("payload", int(2, 3))],
        &[("variant", variant.clone())]
    ));
    assert!(!Fixture::new().check(
        operation(40, &["Shape", "size"]),
        &[("payload", int(0, 101))],
        &[("variant", variant.clone())]
    ));
    assert!(!Fixture::new().check(
        operation(40, &["Shape", "gone"]),
        &[("payload", int(0, 1))],
        &[("variant", variant.clone())]
    ));
    assert!(Fixture::new().check(
        operation(41, &["size"]),
        &[("variant", variant.clone())],
        &[("payload", SemanticType::Option(Box::new(int(-1, 101))))]
    ));
    assert!(!Fixture::new().check(
        operation(41, &["size"]),
        &[("variant", variant)],
        &[("payload", SemanticType::Option(Box::new(int(0, 99))))]
    ));
}
fn match_operation(tags: &[&str], default: &str) -> Vec<u8> {
    let mut b = vec![5];
    b.extend_from_slice(&(tags.len() as u32).to_le_bytes());
    for tag in tags {
        string(&mut b, tag);
        string(&mut b, "branch");
    }
    string(&mut b, default);
    b
}
#[test]
fn schema_match_preserves_rust_tags_and_exact_named_interfaces() {
    let make = || {
        let mut f = Fixture::new();
        f.branch(
            "branch",
            &[("payload", SemanticType::Text)],
            &[("value", SemanticType::Text)],
        );
        f
    };
    let inputs = [
        ("selector", SemanticType::Variant("Shape".into())),
        ("payload", SemanticType::Text),
    ];
    let outputs = [("value", SemanticType::Text)];
    assert!(make().check(
        match_operation(&["unknown-but-nonempty"], "branch"),
        &inputs,
        &outputs
    ));
    assert!(!make().check(
        match_operation(&["name", "name"], "branch"),
        &inputs,
        &outputs
    ));
    assert!(!make().check(match_operation(&[""], "branch"), &inputs, &outputs));
    assert!(!make().check(match_operation(&["name"], ""), &inputs, &outputs));
    assert!(!make().check(
        match_operation(&["name"], "branch"),
        &inputs,
        &[("wrong", SemanticType::Text)]
    ));
    assert!(!make().check(
        match_operation(&["name"], "branch"),
        &[
            ("selector", SemanticType::Variant("Other".into())),
            ("payload", SemanticType::Text)
        ],
        &outputs
    ));
    assert!(!make().check(
        match_operation(&["name"], "branch"),
        &[
            ("selector", SemanticType::Bool),
            ("payload", SemanticType::Text)
        ],
        &outputs
    ));
    assert!(!make().check(
        match_operation(&["name"], "branch"),
        &[
            ("selector", SemanticType::Variant("Shape".into())),
            ("wrong", SemanticType::Text)
        ],
        &outputs
    ));
}
