use std::collections::BTreeSet;

use g0::gir::{
    Edge, Graph, IntegerType, Literal, Node, Operation, Port, SemanticType,
    SourceEndpoint, TargetEndpoint,
};
use g0::machine::MachineProfile;
use g0::native_program::compile_program;
use g0::program::{PlatformContract, ProgramContract};

fn int(min: i128, max: i128) -> SemanticType {
    SemanticType::Integer(IntegerType::new(min, max).unwrap())
}

fn arithmetic_worker(name: &str, operation: Operation, output: SemanticType) -> Graph {
    let mut graph = Graph::new(name);
    graph.inputs = vec![
        Port {
            id: 0,
            name: "numerator".into(),
            ty: int(0, 100),
        },
        Port {
            id: 1,
            name: "divisor".into(),
            ty: int(1, 100),
        },
    ];
    graph.outputs = vec![Port {
        id: 0,
        name: "value".into(),
        ty: output.clone(),
    }];
    graph.nodes.push(Node {
        id: 1,
        operation,
        inputs: graph.inputs.clone(),
        outputs: graph.outputs.clone(),
        effects: BTreeSet::new(),
        required_capabilities: BTreeSet::new(),
    });
    graph.edges = vec![
        Edge {
            from: SourceEndpoint::GraphInput(0),
            to: TargetEndpoint::NodeInput { node: 1, port: 0 },
        },
        Edge {
            from: SourceEndpoint::GraphInput(1),
            to: TargetEndpoint::NodeInput { node: 1, port: 1 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    graph
}

fn main_graph() -> Graph {
    let mut graph = Graph::new("main");
    graph.outputs = vec![Port {
        id: 0,
        name: "answer".into(),
        ty: int(0, 199),
    }];
    graph.nodes = vec![
        Node {
            id: 1,
            operation: Operation::Const(Literal::Integer(100)),
            inputs: vec![],
            outputs: vec![Port {
                id: 0,
                name: "numerator".into(),
                ty: int(100, 100),
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        },
        Node {
            id: 2,
            operation: Operation::Const(Literal::Integer(7)),
            inputs: vec![],
            outputs: vec![Port {
                id: 0,
                name: "divisor".into(),
                ty: int(7, 7),
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        },
        Node {
            id: 3,
            operation: Operation::Subgraph("divide".into()),
            inputs: vec![
                Port {
                    id: 0,
                    name: "numerator".into(),
                    ty: int(0, 100),
                },
                Port {
                    id: 1,
                    name: "divisor".into(),
                    ty: int(1, 100),
                },
            ],
            outputs: vec![Port {
                id: 0,
                name: "quotient".into(),
                ty: int(0, 100),
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        },
        Node {
            id: 4,
            operation: Operation::Subgraph("remainder".into()),
            inputs: vec![
                Port {
                    id: 0,
                    name: "numerator".into(),
                    ty: int(0, 100),
                },
                Port {
                    id: 1,
                    name: "divisor".into(),
                    ty: int(1, 100),
                },
            ],
            outputs: vec![Port {
                id: 0,
                name: "remainder".into(),
                ty: int(0, 99),
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        },
        Node {
            id: 5,
            operation: Operation::Add,
            inputs: vec![
                Port {
                    id: 0,
                    name: "quotient".into(),
                    ty: int(0, 100),
                },
                Port {
                    id: 1,
                    name: "remainder".into(),
                    ty: int(0, 99),
                },
            ],
            outputs: vec![Port {
                id: 0,
                name: "answer".into(),
                ty: int(0, 199),
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        },
    ];

    graph.edges = vec![
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::NodeInput { node: 3, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
            to: TargetEndpoint::NodeInput { node: 3, port: 1 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::NodeInput { node: 4, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
            to: TargetEndpoint::NodeInput { node: 4, port: 1 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 3, port: 0 },
            to: TargetEndpoint::NodeInput { node: 5, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 4, port: 0 },
            to: TargetEndpoint::NodeInput { node: 5, port: 1 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 5, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];

    graph
}

fn main() {
    let program = ProgramContract {
        entry_graph: Some("main".into()),
        graphs: vec![
            main_graph(),
            arithmetic_worker("divide", Operation::Div, int(0, 100)),
            arithmetic_worker("remainder", Operation::Rem, int(0, 99)),
        ],
        ..ProgramContract::default()
    };
    let platform = PlatformContract::bootstrap_x86_64_v3();
    let compiled = compile_program(
        &program,
        &platform,
        MachineProfile::x86_64_v3(),
    )
    .unwrap();

    print!("{}", compiled.assembly);
}
