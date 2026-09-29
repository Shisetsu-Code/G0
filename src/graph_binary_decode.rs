use std::collections::BTreeSet;

use crate::gir::{
    AuthorityMode, BigFloatType, Capability, CapabilityClass, DecimalType,
    Edge, Effect, FloatType, Graph, IntegerType, Literal, MatchArm, Node,
    Operation, Port, SemanticType, SourceEndpoint, TargetEndpoint,
};

const MAGIC: &[u8; 4] = b"G0G\0";
const FORMAT_MAJOR: u16 = 0;
const FORMAT_MINOR: u16 = 1;
const MAX_TYPE_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BinaryDecodeIssue {
    Truncated,
    BadMagic,
    UnsupportedVersion { major: u16, minor: u16 },
    InvalidUtf8,
    InvalidTag { domain: &'static str, tag: u8 },
    LengthOverflow,
    TypeDepthExceeded,
    TrailingBytes,
    InvalidGraph,
    InvalidType,
}

pub fn decode_graph(bytes: &[u8]) -> Result<Graph, BinaryDecodeIssue> {
    let mut reader = Reader::new(bytes);
    if reader.take(4)? != MAGIC {
        return Err(BinaryDecodeIssue::BadMagic);
    }
    let major = reader.u16()?;
    let minor = reader.u16()?;
    if major != FORMAT_MAJOR || minor != FORMAT_MINOR {
        return Err(BinaryDecodeIssue::UnsupportedVersion { major, minor });
    }

    let name = reader.string()?;
    let authority = match reader.u8()? {
        0 => AuthorityMode::DefaultDeny,
        tag => {
            return Err(BinaryDecodeIssue::InvalidTag {
                domain: "authority",
                tag,
            })
        }
    };

    let inputs = read_ports(&mut reader)?;
    let outputs = read_ports(&mut reader)?;

    let node_count = reader.len()?;
    let mut nodes = Vec::with_capacity(node_count);
    for _ in 0..node_count {
        let id = reader.u32()?;
        let operation = read_operation(&mut reader)?;
        let node_inputs = read_ports(&mut reader)?;
        let node_outputs = read_ports(&mut reader)?;

        let effect_count = reader.len()?;
        let mut effects = BTreeSet::new();
        for _ in 0..effect_count {
            effects.insert(read_effect(&mut reader)?);
        }

        let capability_count = reader.len()?;
        let mut required_capabilities = BTreeSet::new();
        for _ in 0..capability_count {
            required_capabilities.insert(read_capability(&mut reader)?);
        }

        nodes.push(Node {
            id,
            operation,
            inputs: node_inputs,
            outputs: node_outputs,
            effects,
            required_capabilities,
        });
    }

    let edge_count = reader.len()?;
    let mut edges = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        let from = match reader.u8()? {
            0 => SourceEndpoint::GraphInput(reader.u16()?),
            1 => SourceEndpoint::NodeOutput {
                node: reader.u32()?,
                port: reader.u16()?,
            },
            tag => {
                return Err(BinaryDecodeIssue::InvalidTag {
                    domain: "source endpoint",
                    tag,
                })
            }
        };
        let to = match reader.u8()? {
            0 => TargetEndpoint::NodeInput {
                node: reader.u32()?,
                port: reader.u16()?,
            },
            1 => TargetEndpoint::GraphOutput(reader.u16()?),
            tag => {
                return Err(BinaryDecodeIssue::InvalidTag {
                    domain: "target endpoint",
                    tag,
                })
            }
        };
        edges.push(Edge { from, to });
    }

    if !reader.done() {
        return Err(BinaryDecodeIssue::TrailingBytes);
    }

    let graph = Graph {
        name,
        inputs,
        outputs,
        nodes,
        edges,
        authority,
    };

    crate::gir_validate::validate(&graph)
        .map_err(|_| BinaryDecodeIssue::InvalidGraph)?;
    Ok(graph)
}

fn read_ports(reader: &mut Reader<'_>) -> Result<Vec<Port>, BinaryDecodeIssue> {
    let count = reader.len()?;
    let mut ports = Vec::with_capacity(count);
    for _ in 0..count {
        ports.push(Port {
            id: reader.u16()?,
            name: reader.string()?,
            ty: read_type(reader, 0)?,
        });
    }
    Ok(ports)
}

fn read_type(
    reader: &mut Reader<'_>,
    depth: usize,
) -> Result<SemanticType, BinaryDecodeIssue> {
    if depth > MAX_TYPE_DEPTH {
        return Err(BinaryDecodeIssue::TypeDepthExceeded);
    }
    let next = depth + 1;
    Ok(match reader.u8()? {
        0 => SemanticType::Bool,
        1 => SemanticType::Integer(
            IntegerType::new(reader.i128()?, reader.i128()?)
                .map_err(|_| BinaryDecodeIssue::InvalidType)?,
        ),
        2 => SemanticType::BigInteger,
        3 => SemanticType::Rational,
        4 => SemanticType::Decimal(
            DecimalType::new(reader.u32()?, reader.i32()?)
                .map_err(|_| BinaryDecodeIssue::InvalidType)?,
        ),
        5 => {
            let max_relative_error_ppb = match reader.u8()? {
                0 => None,
                1 => Some(reader.u64()?),
                tag => {
                    return Err(BinaryDecodeIssue::InvalidTag {
                        domain: "float optional error",
                        tag,
                    })
                }
            };
            SemanticType::Float(FloatType {
                max_relative_error_ppb,
            })
        }
        6 => SemanticType::BigFloat(
            BigFloatType::new(reader.u32()?)
                .map_err(|_| BinaryDecodeIssue::InvalidType)?,
        ),
        7 => SemanticType::Text,
        8 => SemanticType::Bytes,
        9 => {
            let len = reader.usize_u64()?;
            SemanticType::Array(
                Box::new(read_type(reader, next)?),
                len,
            )
        },
        10 => SemanticType::Slice(Box::new(read_type(reader, next)?)),
        11 => {
            let len = reader.usize_u64()?;
            SemanticType::Vector(
                Box::new(read_type(reader, next)?),
                len,
            )
        },
        12 => SemanticType::Record(reader.string()?),
        13 => SemanticType::Variant(reader.string()?),
        14 => SemanticType::Option(Box::new(read_type(reader, next)?)),
        15 => SemanticType::Result(
            Box::new(read_type(reader, next)?),
            Box::new(read_type(reader, next)?),
        ),
        16 => SemanticType::Reference(reader.string()?),
        17 => SemanticType::Secret(Box::new(read_type(reader, next)?)),
        18 => SemanticType::Credential(Box::new(read_type(reader, next)?)),
        19 => SemanticType::Unique(Box::new(read_type(reader, next)?)),
        20 => SemanticType::Borrow(Box::new(read_type(reader, next)?)),
        21 => SemanticType::Shared(Box::new(read_type(reader, next)?)),
        22 => SemanticType::State(Box::new(read_type(reader, next)?)),
        23 => SemanticType::Atomic(Box::new(read_type(reader, next)?)),
        24 => SemanticType::Versioned(Box::new(read_type(reader, next)?)),
        tag => {
            return Err(BinaryDecodeIssue::InvalidTag {
                domain: "semantic type",
                tag,
            })
        }
    })
}

fn read_operation(reader: &mut Reader<'_>) -> Result<Operation, BinaryDecodeIssue> {
    Ok(match reader.u8()? {
        0 => Operation::Const(read_literal(reader)?),
        1 => Operation::Add,
        2 => Operation::Sub,
        3 => Operation::Mul,
        26 => Operation::Div,
        27 => Operation::Rem,
        17 => Operation::Eq,
        18 => Operation::Lt,
        19 => Operation::Le,
        20 => Operation::Gt,
        21 => Operation::Ge,
        22 => Operation::And,
        23 => Operation::Or,
        24 => Operation::Xor,
        25 => Operation::Not,
        28 => Operation::ConvertChecked,
        4 => Operation::Select {
            when_true: reader.string()?,
            when_false: reader.string()?,
        },
        5 => {
            let count = reader.len()?;
            let mut arms = Vec::with_capacity(count);
            for _ in 0..count {
                arms.push(MatchArm {
                    tag: reader.string()?,
                    graph: reader.string()?,
                });
            }
            Operation::Match {
                arms,
                default: reader.string()?,
            }
        }
        6 => Operation::Loop {
            condition: reader.string()?,
            body: reader.string()?,
            max_iterations: reader.u64()?,
        },
        7 => Operation::Subgraph(reader.string()?),
        8 => Operation::Import(reader.string()?),
        9 => Operation::Instantiate(reader.string()?),
        10 => Operation::StoreRead {
            resource: reader.string()?,
            fields: reader.strings()?,
        },
        11 => Operation::StoreCreate {
            resource: reader.string()?,
            fields: reader.strings()?,
        },
        12 => Operation::StoreUpdate {
            resource: reader.string()?,
            fields: reader.strings()?,
        },
        13 => Operation::StoreDelete {
            resource: reader.string()?,
        },
        14 => Operation::StoreEnumerate {
            resource: reader.string()?,
        },
        15 => Operation::LocalExecute(reader.string()?),
        16 => Operation::RemoteExecute {
            target: reader.string()?,
            artifact: reader.string()?,
        },
        tag => {
            return Err(BinaryDecodeIssue::InvalidTag {
                domain: "operation",
                tag,
            })
        }
    })
}

fn read_literal(reader: &mut Reader<'_>) -> Result<Literal, BinaryDecodeIssue> {
    Ok(match reader.u8()? {
        0 => match reader.u8()? {
            0 => Literal::Bool(false),
            1 => Literal::Bool(true),
            tag => {
                return Err(BinaryDecodeIssue::InvalidTag {
                    domain: "bool literal",
                    tag,
                })
            }
        },
        1 => Literal::Integer(reader.i128()?),
        2 => Literal::Text(reader.string()?),
        3 => {
            let len = reader.len()?;
            Literal::Bytes(reader.take(len)?.to_vec())
        }
        tag => {
            return Err(BinaryDecodeIssue::InvalidTag {
                domain: "literal",
                tag,
            })
        }
    })
}

fn read_effect(reader: &mut Reader<'_>) -> Result<Effect, BinaryDecodeIssue> {
    Ok(match reader.u8()? {
        0 => Effect::MemoryWrite,
        1 => Effect::Storage,
        2 => Effect::Network,
        3 => Effect::Clock,
        4 => Effect::Entropy,
        5 => Effect::Device,
        6 => Effect::Process,
        7 => Effect::LocalExecution,
        8 => Effect::RemoteExecution,
        9 => Effect::Accelerator,
        10 => Effect::Audit,
        tag => {
            return Err(BinaryDecodeIssue::InvalidTag {
                domain: "effect",
                tag,
            })
        }
    })
}

fn read_capability(
    reader: &mut Reader<'_>,
) -> Result<Capability, BinaryDecodeIssue> {
    let class = match reader.u8()? {
        0 => CapabilityClass::Resource,
        1 => CapabilityClass::Storage,
        2 => CapabilityClass::Network,
        3 => CapabilityClass::Clock,
        4 => CapabilityClass::Entropy,
        5 => CapabilityClass::Device,
        6 => CapabilityClass::Process,
        7 => CapabilityClass::LocalExecution,
        8 => CapabilityClass::RemoteExecution,
        9 => CapabilityClass::Accelerator,
        10 => CapabilityClass::Audit,
        tag => {
            return Err(BinaryDecodeIssue::InvalidTag {
                domain: "capability class",
                tag,
            })
        }
    };

    Ok(Capability::new(
        class,
        reader.string()?,
        reader.string()?,
        reader.string()?,
    ))
}

struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn done(&self) -> bool {
        self.cursor == self.bytes.len()
    }

    fn take(&mut self, len: usize) -> Result<&'a [u8], BinaryDecodeIssue> {
        let end = self
            .cursor
            .checked_add(len)
            .ok_or(BinaryDecodeIssue::LengthOverflow)?;
        if end > self.bytes.len() {
            return Err(BinaryDecodeIssue::Truncated);
        }
        let slice = &self.bytes[self.cursor..end];
        self.cursor = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, BinaryDecodeIssue> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, BinaryDecodeIssue> {
        let bytes: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| BinaryDecodeIssue::Truncated)?;
        Ok(u16::from_le_bytes(bytes))
    }

    fn u32(&mut self) -> Result<u32, BinaryDecodeIssue> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| BinaryDecodeIssue::Truncated)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn i32(&mut self) -> Result<i32, BinaryDecodeIssue> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| BinaryDecodeIssue::Truncated)?;
        Ok(i32::from_le_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, BinaryDecodeIssue> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .map_err(|_| BinaryDecodeIssue::Truncated)?;
        Ok(u64::from_le_bytes(bytes))
    }

    fn i128(&mut self) -> Result<i128, BinaryDecodeIssue> {
        let bytes: [u8; 16] = self
            .take(16)?
            .try_into()
            .map_err(|_| BinaryDecodeIssue::Truncated)?;
        Ok(i128::from_le_bytes(bytes))
    }

    fn len(&mut self) -> Result<usize, BinaryDecodeIssue> {
        usize::try_from(self.u32()?)
            .map_err(|_| BinaryDecodeIssue::LengthOverflow)
    }

    fn usize_u64(&mut self) -> Result<usize, BinaryDecodeIssue> {
        usize::try_from(self.u64()?)
            .map_err(|_| BinaryDecodeIssue::LengthOverflow)
    }

    fn string(&mut self) -> Result<String, BinaryDecodeIssue> {
        let len = self.len()?;
        let bytes = self.take(len)?;
        let value = std::str::from_utf8(bytes)
            .map_err(|_| BinaryDecodeIssue::InvalidUtf8)?;
        Ok(value.to_owned())
    }

    fn strings(&mut self) -> Result<Vec<String>, BinaryDecodeIssue> {
        let count = self.len()?;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            values.push(self.string()?);
        }
        Ok(values)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{IntegerType, Node};
    use crate::graph_binary::encode_graph;
    use crate::graph_format::structurally_equal;

    #[test]
    fn canonical_binary_round_trips_graph() {
        let mut graph = Graph::new("roundtrip");
        graph.outputs = vec![Port {
            id: 0,
            name: "answer".into(),
            ty: SemanticType::Integer(
                IntegerType::new(42, 42).unwrap(),
            ),
        }];
        graph.nodes.push(Node {
            id: 1,
            operation: Operation::Const(Literal::Integer(42)),
            inputs: vec![],
            outputs: graph.outputs.clone(),
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        });
        graph.edges.push(Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        });

        let bytes = encode_graph(&graph).unwrap();
        let decoded = decode_graph(&bytes).unwrap();
        assert!(structurally_equal(&graph, &decoded));
    }

    #[test]
    fn array_and_vector_lengths_round_trip_in_encoder_order() {
        let mut graph = Graph::new("types");
        graph.inputs = vec![
            Port {
                id: 0,
                name: "array".into(),
                ty: SemanticType::Array(
                    Box::new(SemanticType::Bool),
                    7,
                ),
            },
            Port {
                id: 1,
                name: "vector".into(),
                ty: SemanticType::Vector(
                    Box::new(SemanticType::Integer(
                        IntegerType::new(0, 255).unwrap(),
                    )),
                    8,
                ),
            },
        ];

        let bytes = encode_graph(&graph).unwrap();
        let decoded = decode_graph(&bytes).unwrap();
        assert!(structurally_equal(&graph, &decoded));
    }

    fn valid_operation_graph(
        name: &str,
        operation: Operation,
    ) -> Graph {
        let comparison = matches!(
            operation,
            Operation::Eq
                | Operation::Lt
                | Operation::Le
                | Operation::Gt
                | Operation::Ge
        );
        let unary = matches!(operation, Operation::Not);

        let input_ty = if comparison {
            SemanticType::Integer(IntegerType::new(0, 10).unwrap())
        } else {
            SemanticType::Bool
        };
        let input_count = if unary { 1 } else { 2 };

        let inputs: Vec<Port> = (0..input_count)
            .map(|id| Port {
                id,
                name: format!("in{id}"),
                ty: input_ty.clone(),
            })
            .collect();

        let output = Port {
            id: 0,
            name: "out".into(),
            ty: SemanticType::Bool,
        };

        let mut graph = Graph::new(name);
        graph.inputs = inputs.clone();
        graph.outputs = vec![output.clone()];
        graph.nodes.push(Node {
            id: 1,
            operation,
            inputs,
            outputs: vec![output],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        });

        for port in 0..input_count {
            graph.edges.push(Edge {
                from: SourceEndpoint::GraphInput(port),
                to: TargetEndpoint::NodeInput { node: 1, port },
            });
        }
        graph.edges.push(Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        });
        graph
    }

    #[test]
    fn comparison_and_boolean_operations_round_trip() {
        for (index, operation) in [
            Operation::Eq,
            Operation::Lt,
            Operation::Le,
            Operation::Gt,
            Operation::Ge,
            Operation::And,
            Operation::Or,
            Operation::Xor,
            Operation::Not,
        ]
        .into_iter()
        .enumerate()
        {
            let graph =
                valid_operation_graph(&format!("op-{index}"), operation);
            let bytes = encode_graph(&graph).unwrap();
            let decoded = decode_graph(&bytes).unwrap();
            assert!(structurally_equal(&graph, &decoded));
        }
    }

    #[test]
    fn structured_control_operations_round_trip() {
        let operations = vec![
            Operation::Select {
                when_true: "yes".into(),
                when_false: "no".into(),
            },
            Operation::Match {
                arms: vec![
                    MatchArm {
                        tag: "A".into(),
                        graph: "a".into(),
                    },
                    MatchArm {
                        tag: "B".into(),
                        graph: "b".into(),
                    },
                ],
                default: "other".into(),
            },
            Operation::Loop {
                condition: "cond".into(),
                body: "body".into(),
                max_iterations: 100,
            },
        ];

        for (index, operation) in operations.into_iter().enumerate() {
            let mut graph = Graph::new(format!("control-{index}"));
            graph.nodes.push(Node {
                id: 1,
                operation,
                inputs: vec![],
                outputs: vec![],
                effects: BTreeSet::new(),
                required_capabilities: BTreeSet::new(),
            });

            let bytes = encode_graph(&graph).unwrap();
            let decoded = decode_graph(&bytes).unwrap();
            assert!(structurally_equal(&graph, &decoded));
        }
    }

    #[test]
    fn trailing_bytes_are_rejected() {
        let bytes = crate::graph_binary::encode_graph(&Graph::new("g"))
            .unwrap();
        let mut corrupted = bytes;
        corrupted.push(0);

        assert_eq!(
            decode_graph(&corrupted),
            Err(BinaryDecodeIssue::TrailingBytes)
        );
    }
}
