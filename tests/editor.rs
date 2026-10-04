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
