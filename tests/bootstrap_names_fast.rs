use g0::{
    bootstrap_compiler::{compiler_document, compiler_limits},
    execution::{ExecutionLimits, Executor},
    value::Value,
};

fn fixture(names: &[String], wanted: &str) -> Vec<Value> {
    let mut bytes = vec![9, 9, 9];
    let mut rows = Vec::new();
    for name in names {
        let offset = bytes.len();
        bytes.extend_from_slice(&(name.len() as u32).to_le_bytes());
        bytes.extend_from_slice(name.as_bytes());
        rows.push(Value::Array(
            vec![
                Value::Integer(0),
                Value::Integer(0),
                Value::Integer(offset as i128),
                Value::Integer(0),
                Value::Integer(0),
                Value::Integer(0),
                Value::Integer(0),
            ]
            .into(),
        ));
    }
    let wanted_at = bytes.len();
    bytes.extend_from_slice(&(wanted.len() as u32).to_le_bytes());
    bytes.extend_from_slice(wanted.as_bytes());
    vec![
        Value::Bytes(bytes.into()),
        Value::Array(rows.into()),
        Value::Integer(wanted_at as i128),
    ]
}

#[test]
fn canonical_names_require_nonempty_strict_utf8_order() {
    let doc = compiler_document();
    let contract = doc.validated_contract().unwrap();
    for (names, expected) in [
        (vec![], false),
        (vec![""], false),
        (vec!["a"], true),
        (vec!["a", "a"], false),
        (vec!["b", "a"], false),
        (vec!["a", "aa", "ab", "z", "ñ", "😀"], true),
        (vec!["a", ""], false),
        (vec!["ñ", "z"], false),
    ] {
        let names = names.into_iter().map(str::to_string).collect::<Vec<_>>();
        let mut args = fixture(&names, "");
        args.pop();
        assert_eq!(
            Executor::new(&contract, compiler_limits())
                .unwrap()
                .run_graph("validator-canonical-names", args)
                .unwrap(),
            vec![Value::Bool(expected)]
        );
    }
}

#[test]
fn binary_name_lookup_finds_prefix_unicode_and_missing_names() {
    let doc = compiler_document();
    let contract = doc.validated_contract().unwrap();
    let names = ["a", "aa", "ab", "z", "ñ", "😀"].map(str::to_string);
    for wanted in ["", "a", "aa", "aaa", "ab", "b", "z", "ñ", "😀", "😁"] {
        let expected = names
            .iter()
            .position(|s| s == wanted)
            .unwrap_or(names.len());
        assert_eq!(
            Executor::new(&contract, compiler_limits())
                .unwrap()
                .run_graph("validator-name-index-fast", fixture(&names, wanted))
                .unwrap(),
            vec![Value::Integer(expected as i128)]
        );
    }
    assert_eq!(
        Executor::new(&contract, compiler_limits())
            .unwrap()
            .run_graph("validator-name-index-fast", fixture(&[], "a"))
            .unwrap(),
        vec![Value::Integer(0)]
    );
}

#[test]
fn name_lookup_uses_logarithmic_search_under_a_small_quota() {
    let doc = compiler_document();
    let contract = doc.validated_contract().unwrap();
    let names = (0..1024).map(|i| format!("g{i:04}")).collect::<Vec<_>>();
    let limits = ExecutionLimits {
        max_steps: 6000,
        ..compiler_limits()
    };
    assert_eq!(
        Executor::new(&contract, limits)
            .unwrap()
            .run_graph("validator-name-index-fast", fixture(&names, "g1023"))
            .unwrap(),
        vec![Value::Integer(1023)]
    );
}
