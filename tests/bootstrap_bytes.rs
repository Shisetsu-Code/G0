use g0::{
    bootstrap_compiler::{compiler_document, compiler_limits},
    execution::Executor,
    value::Value,
};

fn emit(bytes: &[u8]) -> String {
    let document = compiler_document();
    let program = document.validated_contract().unwrap();
    let result = Executor::new(&program, compiler_limits())
        .unwrap()
        .run_graph("emit-program-byte-values", vec![Value::Bytes(bytes.into())])
        .unwrap();
    let [Value::Text(text)] = result.as_slice() else {
        panic!("expected assembly byte values")
    };
    text.to_string()
}

#[test]
fn source_byte_emission_preserves_flat_protocol_and_empty_input() {
    assert_eq!(emit(&[]), "");
    assert_eq!(emit(&[0, 17, 255]), "0\n.byte 17\n.byte 255");
}

#[test]
fn source_byte_emission_crosses_chunk_boundaries_without_extra_directives() {
    let mut source = vec![255; 65537];
    source[65535] = 17;
    source[65536] = 0;
    let output = emit(&source);
    assert_eq!(output.split("\n.byte ").count(), source.len());
    assert!(output.starts_with("255\n.byte 255"));
    assert!(output.ends_with("255\n.byte 17\n.byte 0"));
    assert!(!output.ends_with('\n'));
}

#[test]
fn source_byte_emission_exceeds_the_value_element_limit_without_a_giant_array() {
    let source = vec![0; 1_000_001];
    let output = emit(&source);
    assert_eq!(output.len(), 1 + 8 * (source.len() - 1));
    assert_eq!(output.split("\n.byte ").count(), source.len());
    assert!(output.starts_with("0\n.byte 0"));
    assert!(output.ends_with("\n.byte 0"));
}
