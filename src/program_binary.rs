//! Native executable containers. Decoding reconstructs definitions, never
//! execution authority. Version 0.1 carries no external bindings or policies.

use crate::gir::Graph;
use crate::graph_binary::{BinaryGraphIssue, encode_graph};
use crate::graph_binary_decode::{BinaryDecodeIssue, decode_graph};
use crate::program::{PlatformContract, ProgramContract, ProgramIssue, validate_program};

const MAGIC: &[u8; 4] = b"G0P\0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramDocument {
    pub entry_graph: String,
    pub graphs: Vec<Graph>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProgramBinaryIssue {
    BadMagic,
    UnsupportedVersion {
        major: u16,
        minor: u16,
    },
    Truncated,
    LengthOverflow,
    InvalidUtf8,
    TrailingBytes,
    EmptyEntry,
    EntryInterface {
        inputs: usize,
        outputs: usize,
    },
    InvalidProgram(Vec<ProgramIssue>),
    EncodeGraph {
        name: String,
        issue: BinaryGraphIssue,
    },
    DecodeGraph {
        index: usize,
        issue: BinaryDecodeIssue,
    },
}

impl ProgramDocument {
    pub fn validated_contract(&self) -> Result<ProgramContract, ProgramBinaryIssue> {
        if self.entry_graph.is_empty() {
            return Err(ProgramBinaryIssue::EmptyEntry);
        }
        let program = ProgramContract {
            entry_graph: Some(self.entry_graph.clone()),
            graphs: self.graphs.clone(),
            ..ProgramContract::default()
        };
        validate_program(&program, &PlatformContract::bootstrap_x86_64_v3())
            .map_err(ProgramBinaryIssue::InvalidProgram)?;
        let entry = program
            .graphs
            .iter()
            .find(|graph| graph.name == self.entry_graph)
            .expect("validated entry graph");
        if !entry.inputs.is_empty() || entry.outputs.len() != 1 {
            return Err(ProgramBinaryIssue::EntryInterface {
                inputs: entry.inputs.len(),
                outputs: entry.outputs.len(),
            });
        }
        Ok(program)
    }
}

/// Canonical ordering is by graph name; each graph uses the existing canonical
/// encoding. An invalid document is never partially encoded as valid output.
pub fn encode_program(document: &ProgramDocument) -> Result<Vec<u8>, ProgramBinaryIssue> {
    document.validated_contract()?;
    let mut bytes = MAGIC.to_vec();
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&1_u16.to_le_bytes());
    put_blob(&mut bytes, document.entry_graph.as_bytes())?;
    put_length(&mut bytes, document.graphs.len())?;
    let mut graphs: Vec<_> = document.graphs.iter().collect();
    graphs.sort_by(|a, b| a.name.cmp(&b.name));
    for graph in graphs {
        let encoded = encode_graph(graph).map_err(|issue| ProgramBinaryIssue::EncodeGraph {
            name: graph.name.clone(),
            issue,
        })?;
        put_blob(&mut bytes, &encoded)?;
    }
    Ok(bytes)
}

pub fn decode_program(bytes: &[u8]) -> Result<ProgramDocument, ProgramBinaryIssue> {
    let mut reader = Reader { remaining: bytes };
    if reader.take(4)? != MAGIC {
        return Err(ProgramBinaryIssue::BadMagic);
    }
    let major = u16::from_le_bytes(reader.take(2)?.try_into().unwrap());
    let minor = u16::from_le_bytes(reader.take(2)?.try_into().unwrap());
    if (major, minor) != (0, 1) {
        return Err(ProgramBinaryIssue::UnsupportedVersion { major, minor });
    }
    let entry_graph = std::str::from_utf8(reader.blob()?)
        .map_err(|_| ProgramBinaryIssue::InvalidUtf8)?
        .to_owned();
    let count = reader.length()?;
    // Every graph needs at least its four-byte framing, before any allocation.
    if count > reader.remaining.len() / 4 {
        return Err(ProgramBinaryIssue::Truncated);
    }
    let mut graphs = Vec::new();
    for index in 0..count {
        graphs.push(
            decode_graph(reader.blob()?)
                .map_err(|issue| ProgramBinaryIssue::DecodeGraph { index, issue })?,
        );
    }
    if !reader.remaining.is_empty() {
        return Err(ProgramBinaryIssue::TrailingBytes);
    }
    let document = ProgramDocument {
        entry_graph,
        graphs,
    };
    document.validated_contract()?;
    Ok(document)
}

fn put_length(bytes: &mut Vec<u8>, length: usize) -> Result<(), ProgramBinaryIssue> {
    let length = u32::try_from(length).map_err(|_| ProgramBinaryIssue::LengthOverflow)?;
    bytes.extend_from_slice(&length.to_le_bytes());
    Ok(())
}

fn put_blob(bytes: &mut Vec<u8>, blob: &[u8]) -> Result<(), ProgramBinaryIssue> {
    put_length(bytes, blob.len())?;
    bytes.extend_from_slice(blob);
    Ok(())
}

struct Reader<'a> {
    remaining: &'a [u8],
}

impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], ProgramBinaryIssue> {
        if length > self.remaining.len() {
            return Err(ProgramBinaryIssue::Truncated);
        }
        let (value, rest) = self.remaining.split_at(length);
        self.remaining = rest;
        Ok(value)
    }

    fn length(&mut self) -> Result<usize, ProgramBinaryIssue> {
        let length = u32::from_le_bytes(self.take(4)?.try_into().unwrap());
        usize::try_from(length).map_err(|_| ProgramBinaryIssue::LengthOverflow)
    }

    fn blob(&mut self) -> Result<&'a [u8], ProgramBinaryIssue> {
        let length = self.length()?;
        self.take(length)
    }
}
