use g0::{
    bootstrap_compiler::{compiler_document, compiler_limits},
    execution::{ExecutionLimits, Executor, RuntimeError},
    value::Value,
};

fn rows(ids: &[i128]) -> Value {
    Value::Array(
        ids.iter()
            .map(|id| Value::Array(vec![Value::Integer(*id)].into()))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn run(name: &str, args: Vec<Value>, limits: ExecutionLimits) -> Result<Vec<Value>, RuntimeError> {
    let document = compiler_document();
    let program = document.validated_contract().unwrap();
    Executor::new(&program, limits)
        .unwrap()
        .run_graph(name, args)
}

#[test]
fn dense_node_lookup_preserves_sparse_reordered_zero_and_missing_identifiers() {
    for (ids, wanted, expected) in [
        (vec![1, 2, 3], 3, 2),
        (vec![7, 42], 42, 1),
        (vec![3, 1, 2], 3, 0),
        (vec![0, 7], 0, 0),
        (vec![1, 2, 3], 42, 3),
        (vec![], 1, 0),
    ] {
        assert_eq!(
            run(
                "validator-node-index-fast",
                vec![rows(&ids), Value::Integer(wanted)],
                compiler_limits()
            )
            .unwrap(),
            vec![Value::Integer(expected)]
        );
    }
}

#[test]
fn dense_node_lookup_avoids_a_linear_scan_under_a_small_step_quota() {
    let args = vec![rows(&(1..=1000).collect::<Vec<_>>()), Value::Integer(1000)];
    let limits = ExecutionLimits {
        max_steps: 200,
        ..compiler_limits()
    };
    assert_eq!(
        run("validator-node-index-fast", args.clone(), limits).unwrap(),
        vec![Value::Integer(999)]
    );
    assert!(matches!(
        run("emitter-node-index", args, limits),
        Err(RuntimeError::StepLimit)
    ));
}

fn ports(ids: &[u16]) -> Vec<u8> {
    let mut bytes = vec![9, 9, 9];
    bytes.extend_from_slice(&(ids.len() as u32).to_le_bytes());
    for id in ids {
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(b'p');
        bytes.push(0); // Bool
    }
    bytes
}

#[test]
fn fast_port_rank_preserves_nonconsecutive_and_reordered_interfaces() {
    for (ids, wanted, expected) in [
        (vec![0, 1, 2], 2, 2),
        (vec![7, 42], 42, 1),
        (vec![7, 0], 0, 1),
        (vec![0, 1], 42, 2),
        (vec![], 0, 0),
    ] {
        let args = vec![
            Value::Bytes(ports(&ids).into()),
            Value::Integer(3),
            Value::Integer(wanted),
        ];
        assert_eq!(
            run("validator-port-rank-fast", args, compiler_limits()).unwrap(),
            vec![Value::Integer(expected)]
        );
    }
}
