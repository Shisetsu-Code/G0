use g0::{
    editor::{EditorError, GraphEditor},
    gir::*,
    graph_binary::encode_graph,
};

#[test]
fn graphical_edits_preserve_native_graph_identity_and_undo() {
    let mut editor = GraphEditor::new();
    let original = editor.encode().unwrap();
    let added = editor.add_integer(7).unwrap();
    editor
        .connect(
            SourceEndpoint::NodeOutput {
                node: added,
                port: 0,
            },
            TargetEndpoint::GraphOutput(0),
        )
        .unwrap();
    let changed = editor.encode().unwrap();
    assert_ne!(changed, original);
    assert_eq!(
        editor.run().unwrap().values,
        vec![g0::value::Value::Integer(7)]
    );
    editor.undo().unwrap();
    editor.undo().unwrap();
    assert_eq!(editor.encode().unwrap(), original);
    editor.redo().unwrap();
    editor.redo().unwrap();
    assert_eq!(editor.encode().unwrap(), changed);
    let reopened = GraphEditor::decode(&changed).unwrap();
    assert_eq!(reopened.encode().unwrap(), changed);
}

#[test]
fn invalid_edits_are_diagnostic_and_cannot_be_saved_or_executed() {
    let mut editor = GraphEditor::new();
    let add = editor.add_operation(Operation::Add).unwrap();
    assert!(editor.validate().is_err());
    assert!(editor.encode().is_err());
    assert!(editor.run().is_err());
    assert!(matches!(
        editor.connect(
            SourceEndpoint::NodeOutput {
                node: add,
                port: 99
            },
            TargetEndpoint::GraphOutput(0)
        ),
        Err(EditorError::Endpoint)
    ));
    editor.undo().unwrap();
    assert!(editor.validate().is_ok());
}

#[test]
fn definition_only_loading_retains_capability_requirements_without_grants() {
    let mut graph = GraphEditor::new().graph().clone();
    graph.nodes[0].operation = Operation::LocalExecute("artifact".into());
    graph.nodes[0].effects.insert(Effect::LocalExecution);
    graph.nodes[0].required_capabilities.insert(Capability::new(
        CapabilityClass::LocalExecution,
        "execute",
        "artifact",
        "scope",
    ));
    let editor = GraphEditor::decode(&encode_graph(&graph).unwrap()).unwrap();
    assert!(matches!(
        editor.run(),
        Err(EditorError::Runtime(
            g0::execution::RuntimeError::MissingCapability(_)
        ))
    ));
}

#[test]
fn typed_toolbox_executes_boolean_and_text_operations() {
    for (operation, literals, expected, ty) in [
        (
            Operation::And,
            vec![Literal::Bool(true), Literal::Bool(false)],
            g0::value::Value::Bool(false),
            SemanticType::Bool,
        ),
        (
            Operation::TextConcat,
            vec![Literal::Text("G".into()), Literal::Text("0".into())],
            g0::value::Value::Text("G0".into()),
            SemanticType::Text,
        ),
    ] {
        let mut editor = GraphEditor::new();
        editor.set_output_type(0, ty).unwrap();
        let op = editor.add_operation(operation).unwrap();
        for (port, literal) in literals.into_iter().enumerate() {
            let id = editor.add_literal(literal).unwrap();
            editor
                .connect(
                    SourceEndpoint::NodeOutput { node: id, port: 0 },
                    TargetEndpoint::NodeInput {
                        node: op,
                        port: port as u16,
                    },
                )
                .unwrap();
        }
        editor
            .connect(
                SourceEndpoint::NodeOutput { node: op, port: 0 },
                TargetEndpoint::GraphOutput(0),
            )
            .unwrap();
        assert_eq!(editor.run().unwrap().values, vec![expected.clone()]);
        assert_eq!(
            GraphEditor::decode(&editor.encode().unwrap())
                .unwrap()
                .run()
                .unwrap()
                .values,
            vec![expected]
        );
    }
}

#[test]
fn typed_properties_reject_invalid_literal_without_history_mutation() {
    let mut editor = GraphEditor::new();
    let original = editor.encode().unwrap();
    assert!(editor.set_literal_text(1, "not an integer").is_err());
    assert_eq!(editor.encode().unwrap(), original);
    assert!(editor.undo().is_err());
    editor.set_literal_text(1, "123").unwrap();
    assert_eq!(
        editor.run().unwrap().values,
        vec![g0::value::Value::Integer(123)]
    );
}

#[test]
fn layout_roundtrip_is_bounded_and_does_not_change_gir() {
    use g0::editor::EditorLayout;
    let editor = GraphEditor::new();
    let gir = editor.encode().unwrap();
    let mut layout = EditorLayout::default();
    layout.set("main", 1, 350, 200).unwrap();
    layout.set("other", 1, 10, 80).unwrap();
    let restored = EditorLayout::decode(&layout.encode().unwrap()).unwrap();
    assert_eq!(restored.get("main", 1), Some((350, 200)));
    assert_eq!(restored.get("other", 1), Some((10, 80)));
    assert!(layout.set("main", 2, -1, 200).is_err());
    assert!(EditorLayout::decode(b"G0L\0bad").is_err());
    assert_eq!(editor.encode().unwrap(), gir);
}

#[test]
fn graphs_and_typed_ports_are_editable_and_undoable() {
    let mut editor = GraphEditor::new();
    editor.add_graph("helper").unwrap();
    assert_eq!(editor.graph_names(), vec!["main", "helper"]);
    editor.select_graph(1).unwrap();
    assert_eq!(
        editor.run().unwrap().values,
        vec![g0::value::Value::Integer(42)]
    );
    let id = editor.add_operation(Operation::Sub).unwrap();
    editor
        .set_port_type(
            Some(id),
            false,
            0,
            SemanticType::Integer(IntegerType { min: 0, max: 100 }),
        )
        .unwrap();
    assert!(editor.node_properties(id).unwrap().contains("arg0"));
    editor.undo().unwrap();
    editor.undo().unwrap();
    editor.undo().unwrap();
    assert_eq!(editor.graph_names(), vec!["main"]);
}

#[test]
fn toolbox_operation_shapes_validate_when_connected() {
    for op in [
        Operation::Add,
        Operation::Sub,
        Operation::Mul,
        Operation::Div,
        Operation::Rem,
        Operation::Eq,
        Operation::Lt,
        Operation::Le,
        Operation::Gt,
        Operation::Ge,
        Operation::And,
        Operation::Or,
        Operation::Xor,
        Operation::Not,
        Operation::TextConcat,
        Operation::BytesConcat,
        Operation::EncodeUtf8,
        Operation::DecodeUtf8,
        Operation::FormatInteger,
        Operation::ConvertChecked,
        Operation::MakeArray,
        Operation::Index,
        Operation::Length,
        Operation::Some,
        Operation::None,
        Operation::Ok,
        Operation::Err,
        Operation::UnwrapOr,
        Operation::Range,
        Operation::ArrayConcat,
        Operation::BytesSlice,
        Operation::BytesFromArray,
        Operation::TextJoin,
    ] {
        let mut editor = GraphEditor::new();
        let id = editor.add_operation(op.clone()).unwrap();
        let node = editor
            .graph()
            .nodes
            .iter()
            .find(|n| n.id == id)
            .unwrap()
            .clone();
        editor
            .set_output_type(0, node.outputs[0].ty.clone())
            .unwrap();
        for port in node.inputs {
            let source = source_for_type(&mut editor, port.ty);
            editor
                .connect(
                    SourceEndpoint::NodeOutput {
                        node: source,
                        port: 0,
                    },
                    TargetEndpoint::NodeInput {
                        node: id,
                        port: port.id,
                    },
                )
                .unwrap();
        }
        editor
            .connect(
                SourceEndpoint::NodeOutput { node: id, port: 0 },
                TargetEndpoint::GraphOutput(0),
            )
            .unwrap();
        editor
            .validate()
            .unwrap_or_else(|error| panic!("{op:?}: {error:?}"));
        editor
            .run()
            .unwrap_or_else(|error| panic!("{op:?}: {error:?}"));
    }
}

fn source_for_type(editor: &mut GraphEditor, ty: SemanticType) -> NodeId {
    let literal = match &ty {
        SemanticType::Integer(t) => Some(Literal::Integer(t.min.max(0))),
        SemanticType::Bool => Some(Literal::Bool(true)),
        SemanticType::Text => Some(Literal::Text("a".into())),
        SemanticType::Bytes => Some(Literal::Bytes(vec![65])),
        _ => None,
    };
    if let Some(literal) = literal {
        return editor.add_literal(literal).unwrap();
    }
    let (operation, elements) = match &ty {
        SemanticType::Option(t) => (Operation::Some, vec![*t.clone()]),
        SemanticType::Array(t, len) => (Operation::MakeArray, vec![*t.clone(); *len]),
        SemanticType::Slice(t) => (Operation::MakeArray, vec![*t.clone(); 2]),
        _ => panic!("unsupported fixture type {ty:?}"),
    };
    let sources: Vec<_> = elements
        .iter()
        .map(|t| source_for_type(editor, t.clone()))
        .collect();
    let inputs = elements
        .into_iter()
        .enumerate()
        .map(|(i, ty)| Port {
            id: i as u16,
            name: format!("in{i}"),
            ty,
        })
        .collect();
    let id = editor
        .add_typed_node(Node {
            id: 0,
            operation,
            inputs,
            outputs: vec![Port {
                id: 0,
                name: "result".into(),
                ty,
            }],
            effects: Default::default(),
            required_capabilities: Default::default(),
        })
        .unwrap();
    for (port, source) in sources.into_iter().enumerate() {
        editor
            .connect(
                SourceEndpoint::NodeOutput {
                    node: source,
                    port: 0,
                },
                TargetEndpoint::NodeInput {
                    node: id,
                    port: port as u16,
                },
            )
            .unwrap();
    }
    id
}

#[test]
fn debugger_pauses_before_execution_and_step_runs_one_real_node() {
    use std::time::Duration;
    let editor = GraphEditor::new();
    let session = editor.start_debugger().unwrap();
    let control = session.control();
    assert_eq!(
        control.wait_paused(Duration::from_secs(2)),
        Some(("main".into(), 1))
    );
    assert!(control.trace().is_empty());
    control.step();
    let run = session.finish().unwrap();
    assert_eq!(run.values, vec![g0::value::Value::Integer(42)]);
    assert_eq!(control.trace().len(), 1);
}

#[test]
fn debugger_stop_while_paused_prevents_execution() {
    use std::time::Duration;
    let session = GraphEditor::new().start_debugger().unwrap();
    let control = session.control();
    assert!(control.wait_paused(Duration::from_secs(2)).is_some());
    control.stop();
    assert!(matches!(
        session.finish(),
        Err(EditorError::Runtime(g0::execution::RuntimeError::Cancelled))
    ));
    assert!(control.trace().is_empty());
}

#[test]
fn debugger_breakpoint_stops_before_target_with_real_preceding_values() {
    use std::time::Duration;
    let mut editor = GraphEditor::new();
    let seven = editor.add_integer(7).unwrap();
    let sum = editor.add_operation(Operation::Add).unwrap();
    for (node, port) in [(1, 0), (seven, 1)] {
        editor
            .connect(
                SourceEndpoint::NodeOutput { node, port: 0 },
                TargetEndpoint::NodeInput { node: sum, port },
            )
            .unwrap();
    }
    editor
        .connect(
            SourceEndpoint::NodeOutput { node: sum, port: 0 },
            TargetEndpoint::GraphOutput(0),
        )
        .unwrap();
    let session = editor.start_debugger().unwrap();
    let control = session.control();
    assert!(control.wait_paused(Duration::from_secs(2)).is_some());
    control.toggle_breakpoint("main", sum).unwrap();
    control.continue_run();
    assert_eq!(
        control.wait_paused(Duration::from_secs(2)),
        Some(("main".into(), sum))
    );
    assert_eq!(control.trace().len(), 2);
    control.step();
    assert_eq!(
        session.finish().unwrap().values,
        vec![g0::value::Value::Integer(49)]
    );
}

#[test]
fn layout_rejects_duplicate_keys_trailing_data_and_oversized_inputs() {
    use g0::editor::EditorLayout;
    let mut layout = EditorLayout::default();
    layout.set("main", 1, 10, 60).unwrap();
    let bytes = layout.encode().unwrap();
    for len in 0..bytes.len() {
        assert!(EditorLayout::decode(&bytes[..len]).is_err());
    }
    let mut duplicate = bytes.clone();
    duplicate[4..8].copy_from_slice(&2u32.to_le_bytes());
    duplicate.extend_from_slice(&bytes[8..]);
    assert!(EditorLayout::decode(&duplicate).is_err());
    let mut trailing = bytes;
    trailing.push(0);
    assert!(EditorLayout::decode(&trailing).is_err());
    assert!(EditorLayout::decode(&vec![0; 1024 * 1024 + 1]).is_err());
}

#[test]
fn generic_typed_form_validates_aggregate_shape_and_executes() {
    let mut editor = GraphEditor::new();
    let int = SemanticType::Integer(IntegerType {
        min: -100,
        max: 100,
    });
    let ty = SemanticType::Option(Box::new(int.clone()));
    let node = Node {
        id: 999,
        operation: Operation::Some,
        inputs: vec![Port {
            id: 0,
            name: "value".into(),
            ty: int,
        }],
        outputs: vec![Port {
            id: 0,
            name: "option".into(),
            ty: ty.clone(),
        }],
        effects: Default::default(),
        required_capabilities: Default::default(),
    };
    let id = editor.add_typed_node(node.clone()).unwrap();
    assert_ne!(id, 999);
    editor.set_output_type(0, ty).unwrap();
    editor
        .connect(
            SourceEndpoint::NodeOutput { node: 1, port: 0 },
            TargetEndpoint::NodeInput { node: id, port: 0 },
        )
        .unwrap();
    editor
        .connect(
            SourceEndpoint::NodeOutput { node: id, port: 0 },
            TargetEndpoint::GraphOutput(0),
        )
        .unwrap();
    assert_eq!(
        editor.run().unwrap().values,
        vec![g0::value::Value::Option(Some(std::sync::Arc::new(
            g0::value::Value::Integer(42)
        )))]
    );
    let original = editor.encode().unwrap();
    let mut invalid = node;
    invalid.inputs.clear();
    assert!(editor.add_typed_node(invalid).is_err());
    assert_eq!(editor.encode().unwrap(), original);
}

#[cfg(windows)]
#[test]
fn native_window_smoke_saves_executes_renders_and_persists_layout() {
    let path = std::env::temp_dir().join(format!(
        "g0-editor-smoke-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&path).unwrap();
    let bitmap = path.join("preview.bmp");
    let graph = path.join("graph.g0g");
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_g0-editor"))
        .arg("--smoke-test")
        .arg(&bitmap)
        .arg(&graph)
        .status()
        .unwrap();
    assert!(status.success());
    let editor = GraphEditor::decode(&std::fs::read(&graph).unwrap()).unwrap();
    assert_eq!(
        editor.run().unwrap().values,
        vec![g0::value::Value::Integer(49)]
    );
    let pixels = std::fs::read(&bitmap).unwrap();
    assert_eq!(&pixels[..2], b"BM");
    assert!(pixels.len() > 1100 * 700 * 4);
    let layout =
        g0::editor::EditorLayout::decode(&std::fs::read(path.join("graph.g0g.layout")).unwrap())
            .unwrap();
    assert_eq!(layout.get("main", 1), Some((40, 90)));
    for file in [bitmap, graph, path.join("graph.g0g.layout")] {
        std::fs::remove_file(file).unwrap();
    }
    std::fs::remove_dir(path).unwrap();
}

#[test]
fn operation_and_schema_forms_create_native_record_program() {
    let mut editor = GraphEditor::new();
    editor
        .apply_schema_form("name=Point\nversion=1\nfields=1,x,required,int(-100,100)")
        .unwrap();
    let id = editor.apply_node_form("operation=MakeRecord Point x\ninputs=x:int(-100,100)\noutputs=value:record(Point)\neffects=\ncapabilities=").unwrap();
    editor
        .set_output_type(0, SemanticType::Record("Point".into()))
        .unwrap();
    editor
        .connect(
            SourceEndpoint::NodeOutput { node: 1, port: 0 },
            TargetEndpoint::NodeInput { node: id, port: 0 },
        )
        .unwrap();
    editor
        .connect(
            SourceEndpoint::NodeOutput { node: id, port: 0 },
            TargetEndpoint::GraphOutput(0),
        )
        .unwrap();
    let bytes = editor.encode().unwrap();
    assert!(bytes.starts_with(b"G0P\0"));
    assert_eq!(
        editor.run().unwrap().values,
        GraphEditor::decode(&bytes).unwrap().run().unwrap().values
    );
    let before = editor.encode().unwrap();
    assert!(
        editor
            .apply_node_form("operation=Unknown\ninputs=\noutputs=\neffects=\ncapabilities=")
            .is_err()
    );
    assert_eq!(editor.encode().unwrap(), before);
}

#[test]
fn capability_form_declares_requirements_without_granting_them() {
    let mut editor = GraphEditor::new();
    let id = editor.apply_node_form("operation=LocalExecute artifact\ninputs=\noutputs=result:int(-100,100)\neffects=LocalExecution\ncapabilities=LocalExecution,execute,artifact,scope").unwrap();
    editor
        .connect(
            SourceEndpoint::NodeOutput { node: id, port: 0 },
            TargetEndpoint::GraphOutput(0),
        )
        .unwrap();
    assert!(matches!(
        editor.run(),
        Err(EditorError::Runtime(
            g0::execution::RuntimeError::MissingCapability(_)
        ))
    ));
    let restored = GraphEditor::decode(&editor.encode().unwrap()).unwrap();
    assert!(matches!(
        restored.run(),
        Err(EditorError::Runtime(
            g0::execution::RuntimeError::MissingCapability(_)
        ))
    ));
}

#[test]
fn generic_subgraph_form_executes_referenced_native_graph() {
    let mut editor = GraphEditor::new();
    editor.add_graph("helper").unwrap();
    let id = editor.apply_node_form("operation=Subgraph helper\ninputs=\noutputs=result:int(-9223372036854775808,9223372036854775807)\neffects=\ncapabilities=").unwrap();
    editor
        .connect(
            SourceEndpoint::NodeOutput { node: id, port: 0 },
            TargetEndpoint::GraphOutput(0),
        )
        .unwrap();
    assert_eq!(
        editor.run().unwrap().values,
        vec![g0::value::Value::Integer(42)]
    );
    assert!(editor.encode().is_ok());
}

#[test]
fn generic_forms_are_bounded_and_reject_bad_types_without_edits() {
    use g0::editor::parse_type;
    assert_eq!(
        parse_type("result(array(2,int(0,255)),option(text))").unwrap(),
        SemanticType::Result(
            Box::new(SemanticType::Array(
                Box::new(SemanticType::Integer(IntegerType { min: 0, max: 255 })),
                2
            )),
            Box::new(SemanticType::Option(Box::new(SemanticType::Text)))
        )
    );
    assert!(parse_type(&format!("{}bool{}", "option(".repeat(33), ")".repeat(33))).is_err());
    assert!(parse_type("int(5,2)").is_err());
    assert!(parse_type("result(bool)").is_err());
    let mut editor = GraphEditor::new();
    let original = editor.encode().unwrap();
    for text in [
        "operation=Add\noperation=Sub\ninputs=\noutputs=\neffects=\ncapabilities=",
        "operation=Add\ninputs=a:int(9,2)\noutputs=\neffects=\ncapabilities=",
    ] {
        assert!(editor.apply_node_form(text).is_err());
    }
    assert!(editor.apply_node_form(&"x".repeat(8193)).is_err());
    assert!(
        editor
            .apply_schema_form("name=P\nversion=0\nfields=")
            .is_err()
    );
    assert_eq!(editor.encode().unwrap(), original);
    assert!(editor.undo().is_err());
}

#[test]
fn forms_edit_existing_node_and_graph_interfaces_preserving_ids() {
    let mut editor = GraphEditor::new();
    let id = editor
        .apply_node_form(
            "operation=ConstInteger 5\ninputs=\noutputs=value:int(5,5)\neffects=\ncapabilities=",
        )
        .unwrap();
    let text = editor.node_form_text(id).unwrap();
    editor
        .apply_node_form_to(
            id,
            &text
                .replace("ConstInteger 5", "ConstInteger 7")
                .replace("int(5,5)", "int(7,7)"),
        )
        .unwrap();
    editor
        .connect(
            SourceEndpoint::NodeOutput { node: id, port: 0 },
            TargetEndpoint::GraphOutput(0),
        )
        .unwrap();
    assert_eq!(
        editor.run().unwrap().values,
        vec![g0::value::Value::Integer(7)]
    );
    editor
        .apply_graph_form("inputs=input:int(0,10)\noutputs=output:int(0,10)")
        .unwrap();
    assert_eq!(editor.graph().inputs.len(), 1);
    assert_eq!(editor.graph().outputs[0].name, "output");
    assert!(editor.graph_form_text().contains("input:int(0,10)"));
    editor.undo().unwrap();
    assert!(editor.graph().inputs.is_empty());
}

#[test]
fn forms_roundtrip_imported_nonsequential_port_ids() {
    let mut graph = g0::gir::Graph::new("imported");
    let port = g0::gir::Port {
        id: 42,
        name: "answer".into(),
        ty: g0::gir::SemanticType::Bool,
    };
    graph.inputs = vec![port.clone()];
    graph.outputs = vec![g0::gir::Port {
        id: 7,
        ..port.clone()
    }];
    graph.edges = vec![g0::gir::Edge {
        from: SourceEndpoint::GraphInput(42),
        to: TargetEndpoint::GraphOutput(7),
    }];
    let mut editor = GraphEditor::decode(&g0::graph_binary::encode_graph(&graph).unwrap()).unwrap();
    let form = editor.graph_form_text();
    assert!(form.contains("42,answer:bool"), "{form}");
    assert!(form.contains("7,answer:bool"), "{form}");
    editor.apply_graph_form(&form).unwrap();
    assert_eq!(editor.graph(), &graph);
    let id = editor
        .apply_node_form(
            "operation=Not\ninputs=42,input:bool\noutputs=7,output:bool\neffects=\ncapabilities=",
        )
        .unwrap();
    let node = editor.graph().nodes[0].clone();
    let form = editor.node_form_text(id).unwrap();
    editor.apply_node_form_to(id, &form).unwrap();
    assert_eq!(editor.graph().nodes[0], node);
}
