use std::collections::{BTreeMap, BTreeSet};

use crate::gir::Graph;
use crate::graph_binary::encode_graph;
use crate::graph_binary_decode::{decode_graph, BinaryDecodeIssue};

const MAGIC: &[u8; 4] = b"G0M\0";
const MAJOR: u16 = 0;
const MINOR: u16 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GraphModule {
    pub entry_graph: Option<String>,
    pub graphs: BTreeMap<String, Graph>,
    pub external_graphs: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleIssue {
    GraphNameMismatch {
        key: String,
        graph_name: String,
    },
    UnknownEntry(String),
    EntryIsExternal(String),
    LocalExternalCollision(String),
    LengthOverflow,
    GraphEncode(String),
}

pub fn validate_module(
    module: &GraphModule,
) -> Result<(), Vec<ModuleIssue>> {
    let mut issues = Vec::new();

    for (name, graph) in &module.graphs {
        if name != &graph.name {
            issues.push(ModuleIssue::GraphNameMismatch {
                key: name.clone(),
                graph_name: graph.name.clone(),
            });
        }
        if module.external_graphs.contains(name) {
            issues.push(ModuleIssue::LocalExternalCollision(name.clone()));
        }
    }

    if let Some(entry) = &module.entry_graph {
        if module.external_graphs.contains(entry) {
            issues.push(ModuleIssue::EntryIsExternal(entry.clone()));
        } else if !module.graphs.contains_key(entry) {
            issues.push(ModuleIssue::UnknownEntry(entry.clone()));
        }
    }

    if issues.is_empty() {
        Ok(())
    } else {
        Err(issues)
    }
}

pub fn encode_module(
    module: &GraphModule,
) -> Result<Vec<u8>, Vec<ModuleIssue>> {
    validate_module(module)?;

    let mut out = Vec::new();
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&MAJOR.to_le_bytes());
    out.extend_from_slice(&MINOR.to_le_bytes());

    match &module.entry_graph {
        Some(entry) => {
            out.push(1);
            put_string(&mut out, entry)?;
        }
        None => out.push(0),
    }

    put_len(&mut out, module.external_graphs.len())?;
    for name in &module.external_graphs {
        put_string(&mut out, name)?;
    }

    put_len(&mut out, module.graphs.len())?;
    for (name, graph) in &module.graphs {
        put_string(&mut out, name)?;
        let encoded = encode_graph(graph)
            .map_err(|_| vec![ModuleIssue::GraphEncode(name.clone())])?;
        put_len(&mut out, encoded.len())?;
        out.extend_from_slice(&encoded);
    }

    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleDecodeIssue {
    Truncated,
    BadMagic,
    UnsupportedVersion { major: u16, minor: u16 },
    InvalidUtf8,
    InvalidOptionalTag(u8),
    LengthOverflow,
    Graph(BinaryDecodeIssue),
    TrailingBytes,
    InvalidModule(Vec<ModuleIssue>),
}

pub fn decode_module(
    bytes: &[u8],
) -> Result<GraphModule, ModuleDecodeIssue> {
    let mut reader = Reader::new(bytes);
    if reader.take(4)? != MAGIC {
        return Err(ModuleDecodeIssue::BadMagic);
    }

    let major = reader.u16()?;
    let minor = reader.u16()?;
    if major != MAJOR || minor != MINOR {
        return Err(ModuleDecodeIssue::UnsupportedVersion { major, minor });
    }

    let entry_graph = match reader.u8()? {
        0 => None,
        1 => Some(reader.string()?),
        tag => return Err(ModuleDecodeIssue::InvalidOptionalTag(tag)),
    };

    let external_count = reader.len()?;
    let mut external_graphs = BTreeSet::new();
    for _ in 0..external_count {
        external_graphs.insert(reader.string()?);
    }

    let graph_count = reader.len()?;
    let mut graphs = BTreeMap::new();
    for _ in 0..graph_count {
        let name = reader.string()?;
        let len = reader.len()?;
        let graph =
            decode_graph(reader.take(len)?).map_err(ModuleDecodeIssue::Graph)?;
        graphs.insert(name, graph);
    }

    if !reader.done() {
        return Err(ModuleDecodeIssue::TrailingBytes);
    }

    let module = GraphModule {
        entry_graph,
        graphs,
        external_graphs,
    };
    validate_module(&module).map_err(ModuleDecodeIssue::InvalidModule)?;
    Ok(module)
}

fn put_len(out: &mut Vec<u8>, value: usize) -> Result<(), Vec<ModuleIssue>> {
    let value =
        u32::try_from(value).map_err(|_| vec![ModuleIssue::LengthOverflow])?;
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn put_string(
    out: &mut Vec<u8>,
    value: &str,
) -> Result<(), Vec<ModuleIssue>> {
    put_len(out, value.len())?;
    out.extend_from_slice(value.as_bytes());
    Ok(())
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

    fn take(&mut self, len: usize) -> Result<&'a [u8], ModuleDecodeIssue> {
        let end = self
            .cursor
            .checked_add(len)
            .ok_or(ModuleDecodeIssue::LengthOverflow)?;
        if end > self.bytes.len() {
            return Err(ModuleDecodeIssue::Truncated);
        }
        let result = &self.bytes[self.cursor..end];
        self.cursor = end;
        Ok(result)
    }

    fn u8(&mut self) -> Result<u8, ModuleDecodeIssue> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ModuleDecodeIssue> {
        let bytes: [u8; 2] = self
            .take(2)?
            .try_into()
            .map_err(|_| ModuleDecodeIssue::Truncated)?;
        Ok(u16::from_le_bytes(bytes))
    }

    fn len(&mut self) -> Result<usize, ModuleDecodeIssue> {
        usize::try_from(self.u32()?)
            .map_err(|_| ModuleDecodeIssue::LengthOverflow)
    }

    fn u32(&mut self) -> Result<u32, ModuleDecodeIssue> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .map_err(|_| ModuleDecodeIssue::Truncated)?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn string(&mut self) -> Result<String, ModuleDecodeIssue> {
        let len = self.len()?;
        let raw = self.take(len)?;
        let value =
            std::str::from_utf8(raw).map_err(|_| ModuleDecodeIssue::InvalidUtf8)?;
        Ok(value.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_encoding_is_canonical_by_graph_name() {
        let mut a = GraphModule::default();
        a.graphs.insert("b".into(), Graph::new("b"));
        a.graphs.insert("a".into(), Graph::new("a"));

        let mut b = GraphModule::default();
        b.graphs.insert("a".into(), Graph::new("a"));
        b.graphs.insert("b".into(), Graph::new("b"));

        assert_eq!(encode_module(&a).unwrap(), encode_module(&b).unwrap());
    }

    #[test]
    fn module_round_trips_entry_and_external_references() {
        let module = GraphModule {
            entry_graph: Some("main".into()),
            graphs: BTreeMap::from([(
                "main".into(),
                Graph::new("main"),
            )]),
            external_graphs: BTreeSet::from(["host.log".into()]),
        };

        let bytes = encode_module(&module).unwrap();
        assert_eq!(decode_module(&bytes).unwrap(), module);
    }

    #[test]
    fn executable_entry_must_be_local() {
        let module = GraphModule {
            entry_graph: Some("missing".into()),
            ..GraphModule::default()
        };

        assert_eq!(
            validate_module(&module),
            Err(vec![ModuleIssue::UnknownEntry("missing".into())])
        );
    }
}
