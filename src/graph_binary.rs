use crate::gir::{
    AuthorityMode, Capability, CapabilityClass, Effect, Graph, Literal,
    Operation, Port, SemanticType, SourceEndpoint, TargetEndpoint,
};
use crate::graph_format::canonicalize_graph;

const MAGIC: &[u8; 4] = b"G0G\0";
const FORMAT_MAJOR: u16 = 0;
const FORMAT_MINOR: u16 = 1;
const MAX_TYPE_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryGraphIssue {
    InvalidGraph,
    LengthOverflow,
    TypeDepthExceeded,
}

pub fn encode_graph(
    graph: &Graph,
) -> Result<Vec<u8>, BinaryGraphIssue> {
    crate::gir_validate::validate(graph)
        .map_err(|_| BinaryGraphIssue::InvalidGraph)?;

    let document = canonicalize_graph(graph);
    let graph = &document.graph;
    let mut out = Vec::new();

    out.extend_from_slice(MAGIC);
    put_u16(&mut out, FORMAT_MAJOR);
    put_u16(&mut out, FORMAT_MINOR);
    put_string(&mut out, &graph.name)?;
    put_u8(&mut out, authority_tag(graph.authority));

    put_ports(&mut out, &graph.inputs)?;
    put_ports(&mut out, &graph.outputs)?;

    put_len(&mut out, graph.nodes.len())?;
    for node in &graph.nodes {
        put_u32(&mut out, node.id);
        put_operation(&mut out, &node.operation)?;
        put_ports(&mut out, &node.inputs)?;
        put_ports(&mut out, &node.outputs)?;

        put_len(&mut out, node.effects.len())?;
        for effect in &node.effects {
            put_u8(&mut out, effect_tag(*effect));
        }

        put_len(&mut out, node.required_capabilities.len())?;
        for capability in &node.required_capabilities {
            put_capability(&mut out, capability)?;
        }
    }

    put_len(&mut out, graph.edges.len())?;
    for edge in &graph.edges {
        match edge.from {
            SourceEndpoint::GraphInput(port) => {
                put_u8(&mut out, 0);
                put_u16(&mut out, port);
            }
            SourceEndpoint::NodeOutput { node, port } => {
                put_u8(&mut out, 1);
                put_u32(&mut out, node);
                put_u16(&mut out, port);
            }
        }

        match edge.to {
            TargetEndpoint::NodeInput { node, port } => {
                put_u8(&mut out, 0);
                put_u32(&mut out, node);
                put_u16(&mut out, port);
            }
            TargetEndpoint::GraphOutput(port) => {
                put_u8(&mut out, 1);
                put_u16(&mut out, port);
            }
        }
    }

    Ok(out)
}

pub fn diagnostic_fingerprint64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    bytes.iter().fold(OFFSET, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(PRIME)
    })
}

fn put_ports(
    out: &mut Vec<u8>,
    ports: &[Port],
) -> Result<(), BinaryGraphIssue> {
    put_len(out, ports.len())?;
    for port in ports {
        put_u16(out, port.id);
        put_string(out, &port.name)?;
        put_type(out, &port.ty, 0)?;
    }
    Ok(())
}

fn put_type(
    out: &mut Vec<u8>,
    ty: &SemanticType,
    depth: usize,
) -> Result<(), BinaryGraphIssue> {
    if depth > MAX_TYPE_DEPTH {
        return Err(BinaryGraphIssue::TypeDepthExceeded);
    }
    let next = depth + 1;

    match ty {
        SemanticType::Bool => put_u8(out, 0),
        SemanticType::Integer(range) => {
            put_u8(out, 1);
            put_i128(out, range.min);
            put_i128(out, range.max);
        }
        SemanticType::BigInteger => put_u8(out, 2),
        SemanticType::Rational => put_u8(out, 3),
        SemanticType::Decimal(decimal) => {
            put_u8(out, 4);
            put_u32(out, decimal.precision_digits);
            put_i32(out, decimal.scale);
        }
        SemanticType::Float(float) => {
            put_u8(out, 5);
            match float.max_relative_error_ppb {
                Some(value) => {
                    put_u8(out, 1);
                    put_u64(out, value);
                }
                None => put_u8(out, 0),
            }
        }
        SemanticType::BigFloat(big) => {
            put_u8(out, 6);
            put_u32(out, big.precision_bits);
        }
        SemanticType::Text => put_u8(out, 7),
        SemanticType::Bytes => put_u8(out, 8),
        SemanticType::Array(inner, len) => {
            put_u8(out, 9);
            put_u64(
                out,
                u64::try_from(*len)
                    .map_err(|_| BinaryGraphIssue::LengthOverflow)?,
            );
            put_type(out, inner, next)?;
        }
        SemanticType::Slice(inner) => {
            put_u8(out, 10);
            put_type(out, inner, next)?;
        }
        SemanticType::Vector(inner, len) => {
            put_u8(out, 11);
            put_u64(
                out,
                u64::try_from(*len)
                    .map_err(|_| BinaryGraphIssue::LengthOverflow)?,
            );
            put_type(out, inner, next)?;
        }
        SemanticType::Record(name) => {
            put_u8(out, 12);
            put_string(out, name)?;
        }
        SemanticType::Variant(name) => {
            put_u8(out, 13);
            put_string(out, name)?;
        }
        SemanticType::Option(inner) => {
            put_u8(out, 14);
            put_type(out, inner, next)?;
        }
        SemanticType::Result(ok, err) => {
            put_u8(out, 15);
            put_type(out, ok, next)?;
            put_type(out, err, next)?;
        }
        SemanticType::Reference(name) => {
            put_u8(out, 16);
            put_string(out, name)?;
        }
        SemanticType::Secret(inner) => {
            put_u8(out, 17);
            put_type(out, inner, next)?;
        }
        SemanticType::Credential(inner) => {
            put_u8(out, 18);
            put_type(out, inner, next)?;
        }
        SemanticType::Unique(inner) => {
            put_u8(out, 19);
            put_type(out, inner, next)?;
        }
        SemanticType::Borrow(inner) => {
            put_u8(out, 20);
            put_type(out, inner, next)?;
        }
        SemanticType::Shared(inner) => {
            put_u8(out, 21);
            put_type(out, inner, next)?;
        }
        SemanticType::State(inner) => {
            put_u8(out, 22);
            put_type(out, inner, next)?;
        }
        SemanticType::Atomic(inner) => {
            put_u8(out, 23);
            put_type(out, inner, next)?;
        }
        SemanticType::Versioned(inner) => {
            put_u8(out, 24);
            put_type(out, inner, next)?;
        }
    }

    Ok(())
}

fn put_operation(
    out: &mut Vec<u8>,
    operation: &Operation,
) -> Result<(), BinaryGraphIssue> {
    match operation {
        Operation::Const(literal) => {
            put_u8(out, 0);
            put_literal(out, literal)?;
        }
        Operation::Add => put_u8(out, 1),
        Operation::Sub => put_u8(out, 2),
        Operation::Mul => put_u8(out, 3),
        Operation::Select => put_u8(out, 4),
        Operation::Match => put_u8(out, 5),
        Operation::Loop => put_u8(out, 6),
        Operation::Subgraph(name) => {
            put_u8(out, 7);
            put_string(out, name)?;
        }
        Operation::Import(name) => {
            put_u8(out, 8);
            put_string(out, name)?;
        }
        Operation::Instantiate(name) => {
            put_u8(out, 9);
            put_string(out, name)?;
        }
        Operation::StoreRead { resource, fields } => {
            put_u8(out, 10);
            put_string(out, resource)?;
            put_strings(out, fields)?;
        }
        Operation::StoreCreate { resource, fields } => {
            put_u8(out, 11);
            put_string(out, resource)?;
            put_strings(out, fields)?;
        }
        Operation::StoreUpdate { resource, fields } => {
            put_u8(out, 12);
            put_string(out, resource)?;
            put_strings(out, fields)?;
        }
        Operation::StoreDelete { resource } => {
            put_u8(out, 13);
            put_string(out, resource)?;
        }
        Operation::StoreEnumerate { resource } => {
            put_u8(out, 14);
            put_string(out, resource)?;
        }
        Operation::LocalExecute(name) => {
            put_u8(out, 15);
            put_string(out, name)?;
        }
        Operation::RemoteExecute { target, artifact } => {
            put_u8(out, 16);
            put_string(out, target)?;
            put_string(out, artifact)?;
        }
    }
    Ok(())
}

fn put_literal(
    out: &mut Vec<u8>,
    literal: &Literal,
) -> Result<(), BinaryGraphIssue> {
    match literal {
        Literal::Bool(value) => {
            put_u8(out, 0);
            put_u8(out, u8::from(*value));
        }
        Literal::Integer(value) => {
            put_u8(out, 1);
            put_i128(out, *value);
        }
        Literal::Text(value) => {
            put_u8(out, 2);
            put_string(out, value)?;
        }
        Literal::Bytes(value) => {
            put_u8(out, 3);
            put_len(out, value.len())?;
            out.extend_from_slice(value);
        }
    }
    Ok(())
}

fn put_capability(
    out: &mut Vec<u8>,
    capability: &Capability,
) -> Result<(), BinaryGraphIssue> {
    put_u8(out, capability_tag(capability.class));
    put_string(out, &capability.action)?;
    put_string(out, &capability.resource)?;
    put_string(out, &capability.scope)?;
    Ok(())
}

fn put_strings(
    out: &mut Vec<u8>,
    values: &[String],
) -> Result<(), BinaryGraphIssue> {
    put_len(out, values.len())?;
    for value in values {
        put_string(out, value)?;
    }
    Ok(())
}

fn put_string(
    out: &mut Vec<u8>,
    value: &str,
) -> Result<(), BinaryGraphIssue> {
    put_len(out, value.len())?;
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn put_len(out: &mut Vec<u8>, value: usize) -> Result<(), BinaryGraphIssue> {
    let value =
        u32::try_from(value).map_err(|_| BinaryGraphIssue::LengthOverflow)?;
    put_u32(out, value);
    Ok(())
}

fn authority_tag(authority: AuthorityMode) -> u8 {
    match authority {
        AuthorityMode::DefaultDeny => 0,
    }
}

fn effect_tag(effect: Effect) -> u8 {
    effect as u8
}

fn capability_tag(class: CapabilityClass) -> u8 {
    class as u8
}

fn put_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i128(out: &mut Vec<u8>, value: i128) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{IntegerType, Node};

    fn node(id: u32, value: i128) -> Node {
        Node {
            id,
            operation: Operation::Const(Literal::Integer(value)),
            inputs: vec![],
            outputs: vec![Port {
                id: 0,
                name: "out".into(),
                ty: SemanticType::Integer(
                    IntegerType::new(value, value).unwrap(),
                ),
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        }
    }

    #[test]
    fn equivalent_graph_order_has_identical_binary() {
        let mut a = Graph::new("g");
        a.nodes = vec![node(2, 2), node(1, 1)];

        let mut b = Graph::new("g");
        b.nodes = vec![node(1, 1), node(2, 2)];

        assert_eq!(encode_graph(&a).unwrap(), encode_graph(&b).unwrap());
    }

    #[test]
    fn binary_has_stable_magic_and_version() {
        let graph = Graph::new("empty");
        let bytes = encode_graph(&graph).unwrap();

        assert_eq!(&bytes[..4], MAGIC);
        assert_eq!(&bytes[4..6], &FORMAT_MAJOR.to_le_bytes());
        assert_eq!(&bytes[6..8], &FORMAT_MINOR.to_le_bytes());
    }

    #[test]
    fn diagnostic_fingerprint_is_deterministic() {
        let bytes = encode_graph(&Graph::new("empty")).unwrap();
        assert_eq!(
            diagnostic_fingerprint64(&bytes),
            diagnostic_fingerprint64(&bytes)
        );
    }
}
