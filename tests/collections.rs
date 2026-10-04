use g0::{
    execution::{ExecutionLimits, Executor, RuntimeError},
    gir::*,
    program::ProgramContract,
    value::Value,
};
use std::{collections::BTreeSet, sync::Arc};

fn port(id: u16, ty: SemanticType) -> Port {
    Port {
        id,
        name: format!("p{id}"),
        ty,
    }
}
fn node(id: u32, operation: Operation, inputs: Vec<Port>, outputs: Vec<Port>) -> Node {
    Node {
        id,
        operation,
        inputs,
        outputs,
        effects: BTreeSet::new(),
        required_capabilities: BTreeSet::new(),
    }
}
fn fixture() -> ProgramContract {
    let byte = SemanticType::Integer(IntegerType { min: 0, max: 255 });
    let texts = SemanticType::Slice(Box::new(SemanticType::Text));
    let mut item = Graph::new("format-byte");
    item.inputs = vec![port(0, byte.clone())];
    item.outputs = vec![port(0, SemanticType::Text)];
    item.nodes = vec![node(
        1,
        Operation::FormatInteger,
        item.inputs.clone(),
        item.outputs.clone(),
    )];
    item.edges = vec![
        Edge {
            from: SourceEndpoint::GraphInput(0),
            to: TargetEndpoint::NodeInput { node: 1, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    let mut main = Graph::new("main");
    main.inputs = vec![port(0, SemanticType::Bytes)];
    main.outputs = vec![port(0, SemanticType::Text)];
    main.nodes = vec![
        node(
            1,
            Operation::Map {
                body: "format-byte".into(),
            },
            main.inputs.clone(),
            vec![port(0, texts.clone())],
        ),
        node(
            2,
            Operation::Const(Literal::Text(",".into())),
            vec![],
            vec![port(0, SemanticType::Text)],
        ),
        node(
            3,
            Operation::TextJoin,
            vec![port(0, texts), port(1, SemanticType::Text)],
            main.outputs.clone(),
        ),
    ];
    main.edges = vec![
        Edge {
            from: SourceEndpoint::GraphInput(0),
            to: TargetEndpoint::NodeInput { node: 1, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::NodeInput { node: 3, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
            to: TargetEndpoint::NodeInput { node: 3, port: 1 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 3, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    ProgramContract {
        entry_graph: Some("main".into()),
        graphs: vec![main, item],
        ..Default::default()
    }
}
#[test]
fn map_and_join_preserve_order_and_empty_collection() {
    let program = fixture();
    let mut executor = Executor::new(&program, ExecutionLimits::default()).unwrap();
    assert_eq!(
        executor
            .run_graph("main", vec![Value::Bytes(Arc::from([0, 42, 255]))])
            .unwrap(),
        vec![Value::Text("0,42,255".into())]
    );
    assert_eq!(
        executor
            .run_graph("main", vec![Value::Bytes(Arc::from([]))])
            .unwrap(),
        vec![Value::Text("".into())]
    );
    let graph = &program.graphs[0];
    assert_eq!(
        g0::graph_binary_decode::decode_graph(&g0::graph_binary::encode_graph(graph).unwrap())
            .unwrap(),
        *graph
    );
}
#[test]
fn map_checks_body_type_recursion_and_execution_budgets() {
    let mut program = fixture();
    program.graphs[1].inputs[0].ty = SemanticType::Bool;
    assert!(Executor::new(&program, ExecutionLimits::default()).is_err());
    let program = fixture();
    let mut executor = Executor::new(
        &program,
        ExecutionLimits {
            max_steps: 2,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        executor.run_graph("main", vec![Value::Bytes(Arc::from([1, 2, 3]))]),
        Err(RuntimeError::StepLimit)
    );
    let mut recursive = fixture();
    recursive.graphs[0].nodes[0].operation = Operation::Map {
        body: "main".into(),
    };
    assert!(Executor::new(&recursive, ExecutionLimits::default()).is_err());
}
