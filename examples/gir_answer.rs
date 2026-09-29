use std::collections::BTreeSet;

use g0::compiler::compile_graph;
use g0::gir::{
    AuthorityMode, Edge, Graph, IntegerType, Literal, Node, Operation, Port,
    SemanticType, SourceEndpoint, TargetEndpoint,
};
use g0::machine::MachineProfile;

fn integer(min: i128, max: i128) -> SemanticType {
    SemanticType::Integer(IntegerType::new(min, max).unwrap())
}

fn main() {
    let graph = Graph {
        name: "answer".into(),
        inputs: vec![],
        outputs: vec![Port {
            id: 0,
            name: "answer".into(),
            ty: integer(42, 42),
        }],
        nodes: vec![
            Node {
                id: 1,
                operation: Operation::Const(Literal::Integer(20)),
                inputs: vec![],
                outputs: vec![Port {
                    id: 0,
                    name: "left".into(),
                    ty: integer(20, 20),
                }],
                effects: BTreeSet::new(),
                required_capabilities: BTreeSet::new(),
            },
            Node {
                id: 2,
                operation: Operation::Const(Literal::Integer(22)),
                inputs: vec![],
                outputs: vec![Port {
                    id: 0,
                    name: "right".into(),
                    ty: integer(22, 22),
                }],
                effects: BTreeSet::new(),
                required_capabilities: BTreeSet::new(),
            },
            Node {
                id: 3,
                operation: Operation::Add,
                inputs: vec![
                    Port {
                        id: 0,
                        name: "left".into(),
                        ty: integer(20, 20),
                    },
                    Port {
                        id: 1,
                        name: "right".into(),
                        ty: integer(22, 22),
                    },
                ],
                outputs: vec![Port {
                    id: 0,
                    name: "answer".into(),
                    ty: integer(42, 42),
                }],
                effects: BTreeSet::new(),
                required_capabilities: BTreeSet::new(),
            },
        ],
        edges: vec![
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
        ],
        authority: AuthorityMode::DefaultDeny,
    };

    let compiled =
        compile_graph(&graph, MachineProfile::x86_64_v3()).unwrap();
    print!("{}", compiled.assembly);
}
