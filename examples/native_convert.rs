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

fn converter() -> Graph {
    let mut graph = Graph::new("convert");
    graph.inputs = vec![Port {
        id: 0,
        name: "value".into(),
        ty: int(0, 1000),
    }];
    graph.outputs = vec![Port {
        id: 0,
        name: "value".into(),
        ty: int(0, 255),
    }];
    graph.nodes.push(Node {
        id: 1,
        operation: Operation::ConvertChecked,
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
        ty: int(0, 255),
    }];
    graph.nodes = vec![
        Node {
            id: 1,
            operation: Operation::Const(Literal::Integer(200)),
            inputs: vec![],
            outputs: vec![Port {
                id: 0,
                name: "value".into(),
                ty: int(200, 200),
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        },
        Node {
            id: 2,
            operation: Operation::Subgraph("convert".into()),
            inputs: vec![Port {
                id: 0,
                name: "value".into(),
                ty: int(0, 1000),
            }],
            outputs: vec![Port {
                id: 0,
                name: "value".into(),
                ty: int(0, 255),
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        },
    ];
    graph.edges = vec![
        Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::NodeInput { node: 2, port: 0 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 2, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        },
    ];
    graph
}

fn main() {
    let program = ProgramContract {
        entry_graph: Some("main".into()),
        graphs: vec![main_graph(), converter()],
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
