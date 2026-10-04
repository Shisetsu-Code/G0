use g0::{bootstrap_compiler::*,execution::Executor,gir::*,value::Value};
#[test]
fn native_profile_defers_nonempty_schema_registry_until_schema_proofs_exist(){
    let graph=g0::editor::GraphEditor::new().graph().clone();let compiler=compiler_document().validated_contract().unwrap();
    for with_schema in [false,true]{let document=g0::program_binary::ProgramDocument{entry_graph:"main".into(),graphs:vec![graph.clone()],schemas:if with_schema{vec![g0::data_format::DataSchema{name:"unused".into(),version:1,fields:vec![]}]}else{vec![]}};let source=g0::program_binary::encode_program(&document).unwrap();let output=Executor::new(&compiler,compiler_limits()).unwrap().run_graph("validator-empty-schemas",vec![Value::Bytes(source.into())]).unwrap();assert_eq!(output,vec![Value::Bool(!with_schema)]);}
}
#[test]
fn native_type_profile_rejects_nested_named_reference_and_linear_values() {
    let scalar=SemanticType::Integer(IntegerType{min:i128::MIN,max:i128::MAX});
    let contract=compiler_document().validated_contract().unwrap();
    for (ty,expected) in [(SemanticType::Array(Box::new(scalar.clone()),4),true),(SemanticType::Result(Box::new(SemanticType::Bytes),Box::new(SemanticType::Bool)),true),(SemanticType::Slice(Box::new(SemanticType::Unique(Box::new(scalar)))),false),(SemanticType::Record("missing".into()),false),(SemanticType::Reference("missing".into()),false)] {
        let bytes=g0::graph_binary::encode_semantic_type(&ty).unwrap();
        let output=Executor::new(&contract,compiler_limits()).unwrap().run_graph("validator-type-profile",vec![Value::Bytes(bytes.into()),Value::Integer(0)]).unwrap();
        assert_eq!(output,vec![Value::Bool(expected)]);
    }
}
