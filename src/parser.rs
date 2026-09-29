use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::graph::{Graph, Node, NodeId, Op, Type};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug)]
enum RawOp {
    Const(i64),
    Add(String, String),
    Sub(String, String),
    Mul(String, String),
}

#[derive(Debug)]
struct RawNode {
    line: usize,
    name: String,
    ty: Type,
    op: RawOp,
}

fn err(line: usize, message: impl Into<String>) -> ParseError {
    ParseError { line, message: message.into() }
}

fn value_name(token: &str, line: usize) -> Result<String, ParseError> {
    let Some(name) = token.strip_prefix('%') else {
        return Err(err(line, format!("expected value reference beginning with %, got '{token}'")));
    };
    if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return Err(err(line, format!("invalid value name '{token}'")));
    }
    Ok(name.to_owned())
}

pub fn parse(source: &str) -> Result<Graph, ParseError> {
    let mut version: Option<String> = None;
    let mut target = "x86_64-v3".to_owned();
    let mut raw_nodes = Vec::<RawNode>::new();
    let mut declared = BTreeSet::<String>::new();
    let mut output: Option<(usize, String)> = None;

    for (index, original) in source.lines().enumerate() {
        let line_no = index + 1;
        let line = original.split('#').next().unwrap_or("").trim();
        if line.is_empty() { continue; }

        let parts: Vec<&str> = line.split_whitespace().collect();
        match parts.as_slice() {
            ["g0", v] => {
                if version.replace((*v).to_owned()).is_some() {
                    return Err(err(line_no, "duplicate g0 version declaration"));
                }
            }
            ["target", t] => target = (*t).to_owned(),
            ["const", out, "i64", value] => {
                let name = value_name(out, line_no)?;
                if !declared.insert(name.clone()) {
                    return Err(err(line_no, format!("duplicate value %{name}")));
                }
                let value = value.parse::<i64>().map_err(|_| err(line_no, "invalid i64 constant"))?;
                raw_nodes.push(RawNode { line: line_no, name, ty: Type::I64, op: RawOp::Const(value) });
            }
            ["add", out, a, b] | ["sub", out, a, b] | ["mul", out, a, b] => {
                let name = value_name(out, line_no)?;
                if !declared.insert(name.clone()) {
                    return Err(err(line_no, format!("duplicate value %{name}")));
                }
                let a = value_name(a, line_no)?;
                let b = value_name(b, line_no)?;
                let op = match parts[0] {
                    "add" => RawOp::Add(a, b),
                    "sub" => RawOp::Sub(a, b),
                    "mul" => RawOp::Mul(a, b),
                    _ => unreachable!(),
                };
                raw_nodes.push(RawNode { line: line_no, name, ty: Type::I64, op });
            }
            ["return", value] => {
                if output.is_some() {
                    return Err(err(line_no, "duplicate return"));
                }
                output = Some((line_no, value_name(value, line_no)?));
            }
            _ => return Err(err(line_no, format!("unknown or malformed instruction: '{line}'"))),
        }
    }

    let version = version.ok_or_else(|| err(1, "missing 'g0 <version>' declaration"))?;
    if version != "0.1" {
        return Err(err(1, format!("unsupported graph format version '{version}'")));
    }

    let names: BTreeMap<String, NodeId> = raw_nodes.iter().enumerate()
        .map(|(id, n)| (n.name.clone(), id))
        .collect();

    let resolve = |name: &str, line: usize| -> Result<NodeId, ParseError> {
        names.get(name).copied().ok_or_else(|| err(line, format!("unknown value %{name}")))
    };

    let mut nodes = Vec::with_capacity(raw_nodes.len());
    for raw in raw_nodes {
        let op = match raw.op {
            RawOp::Const(v) => Op::Const(v),
            RawOp::Add(a, b) => Op::Add(resolve(&a, raw.line)?, resolve(&b, raw.line)?),
            RawOp::Sub(a, b) => Op::Sub(resolve(&a, raw.line)?, resolve(&b, raw.line)?),
            RawOp::Mul(a, b) => Op::Mul(resolve(&a, raw.line)?, resolve(&b, raw.line)?),
        };
        nodes.push(Node { name: raw.name, ty: raw.ty, op });
    }

    let (line, output_name) = output.ok_or_else(|| err(source.lines().count().max(1), "missing return"))?;
    let output = resolve(&output_name, line)?;

    Ok(Graph { version, target, nodes, output, names })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_forward_references() {
        let src = "g0 0.1\nadd %sum %a %b\nconst %a i64 20\nconst %b i64 22\nreturn %sum\n";
        let graph = parse(src).unwrap();
        assert_eq!(graph.nodes.len(), 3);
    }

    #[test]
    fn rejects_unknown_values() {
        let src = "g0 0.1\nconst %a i64 1\nadd %x %a %missing\nreturn %x\n";
        assert!(parse(src).is_err());
    }
}
