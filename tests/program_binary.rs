use g0::gir::{
    Edge, Graph, IntegerType, Literal, Node, Operation, Port, SemanticType, SourceEndpoint,
    TargetEndpoint,
};
use g0::program_binary::{ProgramBinaryIssue, ProgramDocument, decode_program, encode_program};
use std::collections::BTreeSet;

fn constant(name: &str, value: i128) -> Graph {
    let mut graph = Graph::new(name);
    let output = Port {
        id: 0,
        name: "answer".into(),
        ty: SemanticType::Integer(IntegerType::new(0, 100).unwrap()),
    };
    graph.outputs.push(output.clone());
    graph.nodes.push(Node {
        id: 1,
        operation: Operation::Const(Literal::Integer(value)),
        inputs: vec![],
        outputs: vec![output],
        effects: BTreeSet::new(),
        required_capabilities: BTreeSet::new(),
    });
    graph.edges.push(Edge {
        from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
        to: TargetEndpoint::GraphOutput(0),
    });
    graph
}

fn document() -> ProgramDocument {
    ProgramDocument {
        entry_graph: "main".into(),
        graphs: vec![constant("unused", 7), constant("main", 42)],
    }
}

// Raw framing is deliberate: invalid programs must be tested without passing
// through the encoder that rejects them.
fn raw(entry: &str, graphs: &[Graph]) -> Vec<u8> {
    let mut bytes = b"G0P\0\0\0\x01\0".to_vec();
    bytes.extend_from_slice(&(entry.len() as u32).to_le_bytes());
    bytes.extend_from_slice(entry.as_bytes());
    bytes.extend_from_slice(&(graphs.len() as u32).to_le_bytes());
    for graph in graphs {
        let blob = g0::graph_binary::encode_graph(graph).unwrap();
        bytes.extend_from_slice(&(blob.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&blob);
    }
    bytes
}

#[test]
fn program_roundtrip_is_canonical_independent_of_graph_order() {
    let mut input = document();
    let bytes = encode_program(&input).unwrap();
    input.graphs.reverse();
    assert_eq!(encode_program(&input).unwrap(), bytes);
    let decoded = decode_program(&bytes).unwrap();
    assert_eq!(decoded.entry_graph, "main");
    assert_eq!(
        decoded
            .graphs
            .iter()
            .map(|g| g.name.as_str())
            .collect::<Vec<_>>(),
        vec!["main", "unused"]
    );
    assert_eq!(encode_program(&decoded).unwrap(), bytes);
}

#[test]
fn missing_empty_and_duplicate_entry_definitions_are_rejected() {
    for (entry, graphs) in [
        ("missing", vec![constant("main", 42)]),
        ("", vec![constant("main", 42)]),
        ("main", vec![constant("main", 42), constant("main", 7)]),
        ("main", vec![]),
    ] {
        assert!(decode_program(&raw(entry, &graphs)).is_err());
        assert!(
            encode_program(&ProgramDocument {
                entry_graph: entry.into(),
                graphs
            })
            .is_err()
        );
    }
}

#[test]
fn executable_entry_requires_no_arguments_and_one_result() {
    let mut graph = constant("main", 42);
    graph.inputs = graph.outputs.clone();
    assert!(decode_program(&raw("main", &[graph])).is_err());
    let mut graph = Graph::new("main");
    assert!(decode_program(&raw("main", &[graph.clone()])).is_err());
    graph = constant("main", 42);
    graph.outputs.push(Port {
        id: 1,
        name: "second".into(),
        ..graph.outputs[0].clone()
    });
    graph.edges.push(Edge {
        from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
        to: TargetEndpoint::GraphOutput(1),
    });
    assert!(decode_program(&raw("main", &[graph])).is_err());
}

#[test]
fn malformed_lengths_versions_and_trailing_bytes_are_rejected() {
    let good = encode_program(&document()).unwrap();
    for end in 0..good.len() {
        assert!(
            decode_program(&good[..end]).is_err(),
            "accepted prefix of length {end}"
        );
    }
    let mut bad = good.clone();
    bad.push(0);
    assert!(decode_program(&bad).is_err());
    for offset in [0, 4, 6, 8] {
        let mut bad = good.clone();
        bad[offset] = 255;
        assert!(decode_program(&bad).is_err());
    }
    let mut bad = b"G0P\0\0\0\x01\0\x01\0\0\0m".to_vec();
    bad.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(decode_program(&bad).is_err());
}

#[test]
fn graph_payload_is_validated_and_unknown_calls_rejected() {
    let mut graph = constant("main", 42);
    graph.nodes[0].operation = Operation::Subgraph("missing".into());
    assert!(decode_program(&raw("main", &[graph])).is_err());
    let mut bytes = raw("main", &[constant("main", 42)]);
    // Header(8), entry length(4), entry(4), count(4), blob length(4).
    bytes[24] = 255;
    assert!(decode_program(&bytes).is_err());
}

#[test]
fn nested_graph_counts_are_bounded_before_allocation() {
    for count in [10000_u32, u32::MAX] {
        let mut payload = b"G0G\0\0\0\x01\0\x01\0\0\0m\0".to_vec();
        payload.extend_from_slice(&count.to_le_bytes());
        let mut bytes = b"G0P\0\0\0\x01\0\x01\0\0\0m\x01\0\0\0".to_vec();
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&payload);
        let error = decode_program(&bytes).unwrap_err();
        assert!(matches!(
            error,
            ProgramBinaryIssue::DecodeGraph {
                issue: g0::graph_binary_decode::BinaryDecodeIssue::LengthExceedsInput,
                ..
            }
        ));
    }
}
