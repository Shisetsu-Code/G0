use g0::{bootstrap_compiler::*, execution::Executor, program_binary::*, value::Value};
#[path="support/program_fixture.rs"] mod fixture;

#[test]
fn g0_program_references_reject_missing_call_target_without_host_decoder() {
    let compiler=compiler_document().validated_contract().unwrap();
    for document in [fixture::select_program(),fixture::loop_program()] {
        let source=encode_program(&document).unwrap();
        let output=Executor::new(&compiler,compiler_limits()).unwrap().run_graph("validator-program-references",vec![Value::Bytes(source.into())]).unwrap();
        assert_eq!(output,vec![Value::Bool(true)]);
    }
    let mut source=encode_program(&fixture::call_program()).unwrap();
    for expected in [true,false] {
        if !expected {let at=source.windows(6).position(|b| b==b"worker").unwrap();source[at..at+6].copy_from_slice(b"absent");}
        let output=Executor::new(&compiler,compiler_limits()).unwrap().run_graph("validator-program-references",vec![Value::Bytes(source.clone().into())]).unwrap();
        assert_eq!(output,vec![Value::Bool(expected)]);
    }
}

#[test]
fn g0_program_names_require_distinct_graphs_and_existing_entry() {
    let compiler = compiler_document().validated_contract().unwrap();
    for (duplicate, missing_entry, expected) in [(false,false,true),(true,false,false),(false,true,false)] {
        let first=g0::editor::GraphEditor::new().graph().clone();
        let mut second=first.clone(); second.name="othr".into();
        let document=ProgramDocument{entry_graph:"main".into(),graphs:vec![first,second],schemas:vec![]};
        let mut source=encode_program(&document).unwrap();
        if duplicate {let at=source.windows(4).position(|b| b==b"othr").unwrap();source[at..at+4].copy_from_slice(b"main");}
        if missing_entry {source[12..16].copy_from_slice(b"xxxx");}
        let output=Executor::new(&compiler,compiler_limits()).unwrap().run_graph("validator-program-names",vec![Value::Bytes(source.into())]).unwrap();
        assert_eq!(output,vec![Value::Bool(expected)]);
    }
}
