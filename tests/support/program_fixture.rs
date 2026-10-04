use g0::gir::{
    Edge, Graph, IntegerType, Literal, Node, Operation, Port, SemanticType, SourceEndpoint,
    TargetEndpoint,
};
use g0::program_binary::ProgramDocument;
use std::collections::BTreeSet;

fn port(name: &str, ty: SemanticType) -> Port {
    Port {
        id: 0,
        name: name.into(),
        ty,
    }
}

fn int(min: i128, max: i128) -> SemanticType {
    SemanticType::Integer(IntegerType::new(min, max).unwrap())
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

fn result(id: u32) -> Edge {
    Edge {
        from: SourceEndpoint::NodeOutput { node: id, port: 0 },
        to: TargetEndpoint::GraphOutput(0),
    }
}

fn constant(name: &str, value: i128) -> Graph {
    let mut graph = Graph::new(name);
    graph.outputs = vec![port("value", int(0, 100))];
    graph.nodes.push(node(
        1,
        Operation::Const(Literal::Integer(value)),
        vec![],
        graph.outputs.clone(),
    ));
    graph.edges.push(result(1));
    graph
}

pub fn call_program() -> ProgramDocument {
    let mut main = Graph::new("main");
    main.outputs = vec![port("value", int(0, 100))];
    main.nodes.push(node(
        1,
        Operation::Subgraph("worker".into()),
        vec![],
        main.outputs.clone(),
    ));
    main.edges.push(result(1));
    ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![main, constant("worker", 42)],
    }
}

pub fn select_program() -> ProgramDocument {
    let mut main = Graph::new("main");
    main.outputs = vec![port("value", int(0, 100))];
    main.nodes = vec![
        node(
            1,
            Operation::Const(Literal::Bool(true)),
            vec![],
            vec![port("condition", SemanticType::Bool)],
        ),
        node(
            2,
            Operation::Select {
                when_true: "yes".into(),
                when_false: "no".into(),
            },
            vec![port("condition", SemanticType::Bool)],
            main.outputs.clone(),
        ),
    ];
    main.edges = vec![
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::NodeInput { node: 2, port: 0 },
        },
        result(2),
    ];
    ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![main, constant("yes", 42), constant("no", 7)],
    }
}

pub fn loop_program() -> ProgramDocument {
    let state = port("state", int(0, 10));
    let mut condition = Graph::new("condition");
    condition.inputs = vec![state.clone()];
    condition.outputs = vec![port("continue", SemanticType::Bool)];
    condition.nodes = vec![
        node(
            1,
            Operation::Const(Literal::Integer(0)),
            vec![],
            vec![port("zero", int(0, 0))],
        ),
        node(
            2,
            Operation::Gt,
            vec![
                state.clone(),
                Port {
                    id: 1,
                    name: "zero".into(),
                    ty: int(0, 10),
                },
            ],
            condition.outputs.clone(),
        ),
    ];
    condition.edges = vec![
        Edge {
            from: SourceEndpoint::GraphInput(0),
            to: TargetEndpoint::NodeInput { node: 2, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::NodeInput { node: 2, port: 1 },
        },
        result(2),
    ];
    let mut body = Graph::new("body");
    body.inputs = vec![state.clone()];
    body.outputs = vec![state.clone()];
    body.nodes.push(node(
        1,
        Operation::Const(Literal::Integer(0)),
        vec![],
        body.outputs.clone(),
    ));
    body.edges.push(result(1));
    let mut main = Graph::new("main");
    main.outputs = vec![state.clone()];
    main.nodes = vec![
        node(
            1,
            Operation::Const(Literal::Integer(7)),
            vec![],
            vec![state.clone()],
        ),
        node(
            2,
            Operation::Loop {
                condition: "condition".into(),
                body: "body".into(),
                max_iterations: 3,
            },
            vec![state.clone()],
            vec![state],
        ),
    ];
    main.edges = vec![
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::NodeInput { node: 2, port: 0 },
        },
        result(2),
    ];
    ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![main, condition, body],
    }
}
