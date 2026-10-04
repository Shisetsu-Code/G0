use g0::{
    bootstrap_compiler::{compiler_document, compiler_limits},
    execution::{ExecutionLimits, Executor},
    value::Value,
};

fn string(bytes: &mut Vec<u8>, text: &str) -> i128 {
    let offset = bytes.len() as i128;
    bytes.extend((text.len() as u32).to_le_bytes());
    bytes.extend(text.as_bytes());
    offset
}

fn fixture(names: &[String], wanted: &str) -> Vec<Value> {
    let mut bytes = vec![7; 11];
    let rows = names
        .iter()
        .map(|name| {
            let offset = string(&mut bytes, name);
            Value::Array(
                vec![
                    Value::Integer(0),
                    Value::Integer(0),
                    Value::Integer(offset),
                    Value::Integer(0),
                    Value::Integer(0),
                    Value::Integer(0),
                    Value::Integer(0),
                ]
                .into(),
            )
        })
        .collect::<Vec<_>>();
    let wanted = string(&mut bytes, wanted);
    vec![
        Value::Bytes(bytes.into()),
        Value::Array(rows.into()),
        Value::Integer(wanted),
    ]
}

#[test]
fn comparison_matches_rust_for_unicode_prefixes_and_nul() {
    let contract = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&contract, compiler_limits()).unwrap();
    let names = [
        "", "\0", "a", "a\0", "aa", "ab", "z", "é", "e\u{301}", "β", "中", "😀", "😁",
    ];
    for left in names {
        for right in names {
            let mut bytes = vec![8; 5];
            let a = string(&mut bytes, left);
            let b = string(&mut bytes, right);
            let expected = match left.cmp(right) {
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Less => 1,
                std::cmp::Ordering::Greater => 2,
            };
            assert_eq!(
                executor
                    .run_graph(
                        "validator-name-compare-fast",
                        vec![
                            Value::Bytes(bytes.into()),
                            Value::Integer(a),
                            Value::Integer(b)
                        ]
                    )
                    .unwrap(),
                vec![Value::Integer(expected)],
                "{left:?} / {right:?}"
            );
        }
    }
}

#[test]
fn lookup_and_order_cover_binary_search_boundaries() {
    let contract = compiler_document().validated_contract().unwrap();
    let mut executor = Executor::new(&contract, compiler_limits()).unwrap();
    let mut counts = vec![
        0, 1, 2, 3, 4, 5, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 255, 256, 257, 1023, 1024,
        1025,
    ];
    let mut seed = 0x8bad_f00du32;
    for _ in 0..32 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        counts.push((seed % 2048) as usize);
    }
    for count in counts {
        let names = (0..count)
            .map(|i| format!("β-{i:05}-😀"))
            .collect::<Vec<_>>();
        let mut wanted = vec![String::new(), "α".into(), "γ".into(), "β-00000-😁".into()];
        if count > 0 {
            wanted.extend([
                names[0].clone(),
                names[count / 2].clone(),
                names[count - 1].clone(),
            ]);
        }
        for name in wanted {
            let expected = names.iter().position(|n| n == &name).unwrap_or(count);
            assert_eq!(
                executor
                    .run_graph("validator-name-index-fast", fixture(&names, &name))
                    .unwrap(),
                vec![Value::Integer(expected as i128)],
                "count {count}, wanted {name:?}"
            );
        }
    }
    for (names, expected) in [
        (vec!["a", "aa", "β", "😀"], true),
        (vec!["aa", "a"], false),
        (vec!["a", "β", "β"], false),
        (vec!["a", "β", ""], false),
        (vec!["😀", "β"], false),
    ] {
        let names = names.into_iter().map(str::to_owned).collect::<Vec<_>>();
        let mut args = fixture(&names, "");
        args.pop();
        assert_eq!(
            executor
                .run_graph("validator-canonical-names", args)
                .unwrap(),
            vec![Value::Bool(expected)]
        );
    }
}

#[test]
fn long_shared_prefix_compacts_large_source_before_the_byte_loop() {
    let contract = compiler_document().validated_contract().unwrap();
    let mut bytes = vec![0; 4 * 1024 * 1024];
    let prefix = "a".repeat(128);
    let left = string(&mut bytes, &(prefix.clone() + "b"));
    let right = string(&mut bytes, &(prefix + "c"));
    let mut executor = Executor::new(
        &contract,
        ExecutionLimits {
            max_value_bytes: 64 * 1024 * 1024,
            ..compiler_limits()
        },
    )
    .unwrap();
    assert_eq!(
        executor
            .run_graph(
                "validator-name-compare-fast",
                vec![
                    Value::Bytes(bytes.into()),
                    Value::Integer(left),
                    Value::Integer(right)
                ]
            )
            .unwrap(),
        vec![Value::Integer(1)]
    );
}
