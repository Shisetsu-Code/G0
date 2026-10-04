use g0::{
    execution::{EffectHost, ExecutionLimits},
    gir::*,
    resource_host::ResourceHost,
    value::Value,
};
use std::{collections::BTreeSet, sync::Arc};

fn node(operation: Operation) -> Node {
    Node {
        id: 1,
        operation,
        inputs: vec![],
        outputs: vec![],
        effects: BTreeSet::new(),
        required_capabilities: BTreeSet::new(),
    }
}
fn new_host() -> ResourceHost {
    let program = g0::program::ProgramContract {
        entry_graph: Some("main".into()),
        graphs: vec![g0::editor::GraphEditor::new().graph().clone()],
        ..Default::default()
    };
    ResourceHost::new(
        Arc::new(program),
        ExecutionLimits::default(),
        BTreeSet::new(),
    )
    .unwrap()
}

#[test]
fn child_regions_inherit_secrecy_and_parent_lifetime_in_native_graphs() {
    let mut host = new_host();
    let root = host
        .execute(&node(Operation::RegionOpenSecret), &[Value::Integer(32)])
        .unwrap()
        .remove(0);
    let children = host
        .execute(
            &node(Operation::RegionOpenChild { secret: false }),
            &[root, Value::Integer(16)],
        )
        .unwrap();
    let allocated = host
        .execute(
            &node(Operation::RegionAllocate),
            &[children[1].clone(), Value::Integer(4)],
        )
        .unwrap();
    let written = host
        .execute(
            &node(Operation::RegionWrite),
            &[
                allocated[0].clone(),
                allocated[1].clone(),
                Value::Integer(0),
                Value::Secret(Arc::new(Value::Bytes(Arc::from([42])))),
            ],
        )
        .unwrap();
    let read = host
        .execute(&node(Operation::RegionRead), &written)
        .unwrap();
    assert_eq!(
        read[2],
        Value::Secret(Arc::new(Value::Bytes(Arc::from([42, 0, 0, 0]))))
    );
    assert!(
        g0::value_codec::encode_value(
            &read[2],
            &SemanticType::Secret(Box::new(SemanticType::Bytes)),
            &[],
            Default::default()
        )
        .is_err()
    );
    host.execute(&node(Operation::RegionClose), &children[..1])
        .unwrap();
    assert!(
        host.execute(&node(Operation::RegionRead), &read[..2])
            .is_err()
    );
    let mut secret_program = region_program();
    for graph in &mut secret_program.graphs {
        for port in graph
            .inputs
            .iter_mut()
            .chain(graph.outputs.iter_mut())
            .chain(
                graph
                    .nodes
                    .iter_mut()
                    .flat_map(|n| n.inputs.iter_mut().chain(n.outputs.iter_mut())),
            )
        {
            port.ty = match &port.ty {
                SemanticType::Unique(ty) if matches!(ty.as_ref(),SemanticType::Reference(name) if name == "g0.region") => {
                    SemanticType::Unique(Box::new(SemanticType::Reference(
                        "g0.secret-region".into(),
                    )))
                }
                SemanticType::Unique(ty) if matches!(ty.as_ref(),SemanticType::Reference(name) if name == "g0.buffer") => {
                    SemanticType::Unique(Box::new(SemanticType::Reference(
                        "g0.secret-buffer".into(),
                    )))
                }
                SemanticType::Bytes => SemanticType::Secret(Box::new(SemanticType::Bytes)),
                ty => ty.clone(),
            };
        }
        graph.nodes[1].operation = Operation::RegionOpenSecret;
        let region = graph.nodes[1].outputs[0].clone();
        let mut child = region.clone();
        child.id = 1;
        child.name = "child".into();
        graph.nodes.push(Node {
            id: 9,
            operation: Operation::RegionOpenChild { secret: false },
            inputs: vec![
                region.clone(),
                Port {
                    id: 1,
                    name: "quota".into(),
                    ..graph.nodes[2].outputs[0].clone()
                },
            ],
            outputs: vec![region.clone(), child],
            effects: BTreeSet::from([Effect::MemoryWrite]),
            required_capabilities: BTreeSet::new(),
        });
        graph.nodes.push(Node {
            id: 10,
            operation: Operation::RegionClose,
            inputs: vec![
                region,
                Port {
                    id: 1,
                    name: "closed-child".into(),
                    ..graph.nodes[7].outputs[0].clone()
                },
            ],
            outputs: graph.nodes[7].outputs.clone(),
            effects: BTreeSet::from([Effect::MemoryWrite]),
            required_capabilities: BTreeSet::new(),
        });
        graph.edges.retain(|e| {
            !matches!(e.from, SourceEndpoint::NodeOutput { node: 2, port: 0 })
                && e.to != TargetEndpoint::GraphOutput(1)
        });
        let link = |a, b, c, d| Edge {
            from: SourceEndpoint::NodeOutput { node: a, port: b },
            to: TargetEndpoint::NodeInput { node: c, port: d },
        };
        graph.edges.extend([
            link(2, 0, 9, 0),
            link(3, 0, 9, 1),
            link(9, 1, 4, 0),
            link(9, 0, 10, 0),
            link(8, 0, 10, 1),
            Edge {
                from: SourceEndpoint::NodeOutput { node: 10, port: 0 },
                to: TargetEndpoint::GraphOutput(1),
            },
        ]);
    }
    g0::gir_validate::validate(&secret_program.graphs[0]).unwrap();
    let bytes = g0::graph_binary::encode_graph(&secret_program.graphs[0]).unwrap();
    let mut legacy = bytes.clone();
    legacy[6] = 4;
    assert!(g0::graph_binary_decode::decode_graph(&legacy).is_err());
    secret_program.graphs[0] = g0::graph_binary_decode::decode_graph(&bytes).unwrap();
    let mut host = ResourceHost::new(
        Arc::new(secret_program.clone()),
        Default::default(),
        BTreeSet::new(),
    )
    .unwrap();
    let secret = Value::Secret(Arc::new(Value::Bytes(Arc::from([7]))));
    assert_eq!(
        host.run_graph("regions", vec![secret]).unwrap(),
        vec![
            Value::Secret(Arc::new(Value::Bytes(Arc::from([7, 0, 0, 0])))),
            Value::Bool(true)
        ]
    );
    secret_program.graphs[0].nodes[6].outputs[2].ty = SemanticType::Bytes;
    assert!(
        ResourceHost::new(
            Arc::new(secret_program),
            Default::default(),
            BTreeSet::new()
        )
        .is_err()
    );
}
#[test]
fn graph_resource_handles_move_and_close_without_serialization_or_forgery() {
    let mut host = new_host();
    let region = host
        .execute(&node(Operation::RegionOpen), &[Value::Integer(32)])
        .unwrap()
        .remove(0);
    let mut allocated = host
        .execute(
            &node(Operation::RegionAllocate),
            &[region.clone(), Value::Integer(4)],
        )
        .unwrap();
    assert!(
        host.execute(
            &node(Operation::RegionAllocate),
            &[region, Value::Integer(1)]
        )
        .is_err()
    );
    let buffer = allocated.remove(1);
    let region = allocated.remove(0);
    let mut written = host
        .execute(
            &node(Operation::RegionWrite),
            &[
                region,
                buffer,
                Value::Integer(0),
                Value::Bytes(Arc::from([42])),
            ],
        )
        .unwrap();
    let buffer = written.remove(1);
    let region = written.remove(0);
    let read = host
        .execute(&node(Operation::RegionRead), &[region, buffer])
        .unwrap();
    assert_eq!(read[2], Value::Bytes(Arc::from([42, 0, 0, 0])));
    assert!(
        new_host()
            .execute(&node(Operation::RegionRead), &read[..2])
            .is_err()
    );
    assert_eq!(
        host.execute(&node(Operation::RegionClose), &read[..1])
            .unwrap(),
        vec![Value::Bool(true)]
    );
    assert!(
        host.execute(&node(Operation::RegionRead), &read[..2])
            .is_err()
    );
    assert!(
        g0::value_codec::encode_value(
            &read[1],
            &SemanticType::Unique(Box::new(SemanticType::Reference("g0.buffer".into()))),
            &[],
            Default::default()
        )
        .is_err()
    );
}

#[test]
fn named_records_cannot_hide_linear_handle_fanout() {
    let handle = SemanticType::Unique(Box::new(SemanticType::Reference("g0.buffer".into())));
    let record = SemanticType::Record("HandleBox".into());
    let mut graph = Graph::new("boxed");
    graph.inputs = vec![Port {
        id: 0,
        name: "box".into(),
        ty: record.clone(),
    }];
    graph.outputs = vec![
        Port {
            id: 0,
            name: "first".into(),
            ty: handle.clone(),
        },
        Port {
            id: 1,
            name: "second".into(),
            ty: handle.clone(),
        },
    ];
    for id in [1, 2] {
        graph.nodes.push(Node {
            id,
            operation: Operation::Field {
                name: "buffer".into(),
            },
            inputs: vec![Port {
                id: 0,
                name: "box".into(),
                ty: record.clone(),
            }],
            outputs: vec![Port {
                id: 0,
                name: "buffer".into(),
                ty: handle.clone(),
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        });
        graph.edges.push(Edge {
            from: SourceEndpoint::GraphInput(0),
            to: TargetEndpoint::NodeInput { node: id, port: 0 },
        });
        graph.edges.push(Edge {
            from: SourceEndpoint::NodeOutput { node: id, port: 0 },
            to: TargetEndpoint::GraphOutput((id - 1) as u16),
        });
    }
    let program = g0::program::ProgramContract {
        entry_graph: Some("boxed".into()),
        graphs: vec![graph],
        schemas: vec![g0::data_format::DataSchema {
            name: "HandleBox".into(),
            version: 1,
            fields: vec![g0::data_format::SchemaField {
                tag: 1,
                name: "buffer".into(),
                ty: handle,
                requirement: g0::data_format::FieldRequirement::Required,
            }],
        }],
        ..Default::default()
    };
    assert!(g0::execution::Executor::new(&program, ExecutionLimits::default()).is_err());
}

#[test]
fn region_allocations_check_limits_before_allocation() {
    let mut host = new_host();
    let region = host
        .execute(&node(Operation::RegionOpen), &[Value::Integer(4)])
        .unwrap()
        .remove(0);
    assert!(
        host.execute(
            &node(Operation::RegionAllocate),
            &[region, Value::Integer(i128::MAX)]
        )
        .is_err()
    );
}

#[test]
fn graph_tasks_require_explicit_authority_and_join_once() {
    let body = g0::editor::GraphEditor::new().graph().clone();
    let task_type = SemanticType::Unique(Box::new(SemanticType::Reference("g0.task:main".into())));
    let spawn = Capability::new(CapabilityClass::LocalExecution, "spawn", "main", "tasks");
    let join = Capability::new(CapabilityClass::LocalExecution, "join", "main", "tasks");
    let mut graph = Graph::new("task-driver");
    graph.outputs = body.outputs.clone();
    graph.nodes = vec![
        Node {
            id: 1,
            operation: Operation::TaskSpawn {
                body: "main".into(),
                max_steps: 1000,
                max_value_bytes: 1_000_000,
            },
            inputs: vec![],
            outputs: vec![Port {
                id: 0,
                name: "task".into(),
                ty: task_type.clone(),
            }],
            effects: BTreeSet::from([Effect::LocalExecution]),
            required_capabilities: BTreeSet::from([spawn.clone()]),
        },
        Node {
            id: 2,
            operation: Operation::TaskJoin {
                body: "main".into(),
            },
            inputs: vec![Port {
                id: 0,
                name: "task".into(),
                ty: task_type,
            }],
            outputs: body.outputs.clone(),
            effects: BTreeSet::from([Effect::LocalExecution]),
            required_capabilities: BTreeSet::from([join.clone()]),
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
    let program = Arc::new(g0::program::ProgramContract {
        entry_graph: Some(graph.name.clone()),
        graphs: vec![graph.clone(), body],
        ..Default::default()
    });
    let mut denied =
        ResourceHost::new(program.clone(), ExecutionLimits::default(), BTreeSet::new()).unwrap();
    assert!(matches!(
        denied.run_graph("task-driver", vec![]),
        Err(g0::execution::RuntimeError::MissingCapability(_))
    ));
    let mut host = ResourceHost::new(
        program.clone(),
        ExecutionLimits::default(),
        BTreeSet::from([spawn, join]),
    )
    .unwrap();
    assert_eq!(
        host.run_graph("task-driver", vec![]).unwrap(),
        vec![Value::Integer(42)]
    );
    let task = host.execute(&graph.nodes[0], &[]).unwrap().remove(0);
    assert_eq!(
        host.execute(&graph.nodes[1], std::slice::from_ref(&task))
            .unwrap(),
        vec![Value::Integer(42)]
    );
    assert!(host.execute(&graph.nodes[1], &[task]).is_err());
    let mut starving = graph.nodes[0].clone();
    starving.operation = Operation::TaskSpawn {
        body: "main".into(),
        max_steps: 1,
        max_value_bytes: 1_000_000,
    };
    let task = host.execute(&starving, &[]).unwrap().remove(0);
    assert_eq!(
        host.execute(&graph.nodes[1], &[task]),
        Err(g0::execution::RuntimeError::StepLimit)
    );
    // Order spawning effects, without joining the first child before starting
    // the second. Bool sequencing ports are distinct from child payload inputs.
    let mut parallel = program.as_ref().clone();
    let graph = &mut parallel.graphs[0];
    let ordered = Port {
        id: 1,
        name: "ordered".into(),
        ty: SemanticType::Bool,
    };
    graph.nodes[0].outputs.push(ordered.clone());
    let mut second_spawn = graph.nodes[0].clone();
    second_spawn.id = 3;
    second_spawn.inputs = vec![Port {
        id: 0,
        ..ordered.clone()
    }];
    graph.nodes[1].inputs.push(ordered.clone());
    graph.nodes[1].outputs.push(ordered.clone());
    let mut second_join = graph.nodes[1].clone();
    second_join.id = 4;
    second_join.outputs.pop();
    graph.nodes.extend([second_spawn, second_join]);
    let output = Port {
        id: 1,
        name: "second-result".into(),
        ..graph.outputs[0].clone()
    };
    graph.outputs.push(output);
    let link = |from_node, from_port, to_node, to_port| Edge {
        from: SourceEndpoint::NodeOutput {
            node: from_node,
            port: from_port,
        },
        to: TargetEndpoint::NodeInput {
            node: to_node,
            port: to_port,
        },
    };
    graph.edges.extend([
        link(1, 1, 3, 0),
        link(3, 1, 2, 1),
        link(3, 0, 4, 0),
        link(2, 1, 4, 1),
        Edge {
            from: SourceEndpoint::NodeOutput { node: 4, port: 0 },
            to: TargetEndpoint::GraphOutput(1),
        },
    ]);
    let grants = graph
        .nodes
        .iter()
        .flat_map(|n| n.required_capabilities.clone())
        .collect();
    let mut host =
        ResourceHost::new(Arc::new(parallel), ExecutionLimits::default(), grants).unwrap();
    assert_eq!(
        host.run_graph("task-driver", vec![]).unwrap(),
        vec![Value::Integer(42), Value::Integer(42)]
    );
}

fn region_program() -> g0::program::ProgramContract {
    let region = SemanticType::Unique(Box::new(SemanticType::Reference("g0.region".into())));
    let buffer = SemanticType::Unique(Box::new(SemanticType::Reference("g0.buffer".into())));
    let size = SemanticType::Integer(IntegerType {
        min: 0,
        max: u64::MAX as i128,
    });
    let ports = |types: Vec<SemanticType>| {
        types
            .into_iter()
            .enumerate()
            .map(|(id, ty)| Port {
                id: id as u16,
                name: format!("p{id}"),
                ty,
            })
            .collect()
    };
    let n = |id, operation, inputs, outputs, effect| Node {
        id,
        operation,
        inputs: ports(inputs),
        outputs: ports(outputs),
        effects: if effect {
            BTreeSet::from([Effect::MemoryWrite])
        } else {
            BTreeSet::new()
        },
        required_capabilities: BTreeSet::new(),
    };
    let mut graph = Graph::new("regions");
    graph.inputs = ports(vec![SemanticType::Bytes]);
    graph.outputs = ports(vec![SemanticType::Bytes, SemanticType::Bool]);
    graph.nodes = vec![
        n(
            1,
            Operation::Const(Literal::Integer(64)),
            vec![],
            vec![size.clone()],
            false,
        ),
        n(
            2,
            Operation::RegionOpen,
            vec![size.clone()],
            vec![region.clone()],
            true,
        ),
        n(
            3,
            Operation::Const(Literal::Integer(4)),
            vec![],
            vec![size.clone()],
            false,
        ),
        n(
            4,
            Operation::RegionAllocate,
            vec![region.clone(), size.clone()],
            vec![region.clone(), buffer.clone()],
            true,
        ),
        n(
            5,
            Operation::Const(Literal::Integer(0)),
            vec![],
            vec![size.clone()],
            false,
        ),
        n(
            6,
            Operation::RegionWrite,
            vec![region.clone(), buffer.clone(), size, SemanticType::Bytes],
            vec![region.clone(), buffer.clone()],
            true,
        ),
        n(
            7,
            Operation::RegionRead,
            vec![region.clone(), buffer.clone()],
            vec![region.clone(), buffer, SemanticType::Bytes],
            true,
        ),
        n(
            8,
            Operation::RegionClose,
            vec![region],
            vec![SemanticType::Bool],
            true,
        ),
    ];
    let link = |from_node, from_port, to_node, to_port| Edge {
        from: SourceEndpoint::NodeOutput {
            node: from_node,
            port: from_port,
        },
        to: TargetEndpoint::NodeInput {
            node: to_node,
            port: to_port,
        },
    };
    graph.edges = vec![
        link(1, 0, 2, 0),
        link(2, 0, 4, 0),
        link(3, 0, 4, 1),
        link(4, 0, 6, 0),
        link(4, 1, 6, 1),
        link(5, 0, 6, 2),
        link(6, 0, 7, 0),
        link(6, 1, 7, 1),
        link(7, 0, 8, 0),
        Edge {
            from: SourceEndpoint::GraphInput(0),
            to: TargetEndpoint::NodeInput { node: 6, port: 3 },
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 7, port: 2 },
            to: TargetEndpoint::GraphOutput(0),
        },
        Edge {
            from: SourceEndpoint::NodeOutput { node: 8, port: 0 },
            to: TargetEndpoint::GraphOutput(1),
        },
    ];
    g0::program::ProgramContract {
        entry_graph: Some(graph.name.clone()),
        graphs: vec![graph],
        ..Default::default()
    }
}
#[test]
fn native_graph_executes_region_lifetimes_and_rejects_linear_fanout() {
    let program = region_program();
    let bytes = g0::graph_binary::encode_graph(&program.graphs[0]).unwrap();
    assert_eq!(
        g0::graph_binary::encode_graph(&g0::graph_binary_decode::decode_graph(&bytes).unwrap())
            .unwrap(),
        bytes
    );
    let mut host = ResourceHost::new(
        Arc::new(program.clone()),
        ExecutionLimits::default(),
        BTreeSet::new(),
    )
    .unwrap();
    assert_eq!(
        host.run_graph("regions", vec![Value::Bytes(Arc::from([42]))])
            .unwrap(),
        vec![Value::Bytes(Arc::from([42, 0, 0, 0])), Value::Bool(true)]
    );
    let document = g0::program_binary::ProgramDocument {
        entry_graph: "regions".into(),
        graphs: program.graphs.clone(),
        schemas: vec![],
    };
    let binary = g0::program_binary::encode_program(&document).unwrap();
    assert_eq!(
        g0::native_runtime::execute_embedded(&binary, &[42]).unwrap(),
        vec![Value::Bytes(Arc::from([42, 0, 0, 0])), Value::Bool(true)]
    );
    let mut bad = program;
    bad.graphs[0].outputs.push(Port {
        id: 2,
        name: "stale".into(),
        ty: SemanticType::Unique(Box::new(SemanticType::Reference("g0.region".into()))),
    });
    bad.graphs[0].edges.push(Edge {
        from: SourceEndpoint::NodeOutput { node: 4, port: 0 },
        to: TargetEndpoint::GraphOutput(2),
    });
    assert!(ResourceHost::new(Arc::new(bad), ExecutionLimits::default(), BTreeSet::new()).is_err());
}
