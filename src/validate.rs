use std::fmt;

use crate::graph::{Graph, NodeId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError(pub String);

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ValidationError {}

pub fn validate(graph: &Graph) -> Result<(), ValidationError> {
    if graph.target != "x86_64-v3" {
        return Err(ValidationError(format!(
            "unsupported target '{}'; bootstrap currently supports x86_64-v3 only",
            graph.target
        )));
    }
    if graph.nodes.is_empty() {
        return Err(ValidationError("graph contains no nodes".into()));
    }
    if graph.output >= graph.nodes.len() {
        return Err(ValidationError("return points outside graph".into()));
    }

    for (id, node) in graph.nodes.iter().enumerate() {
        for input in node.op.inputs().into_iter().flatten() {
            if input >= graph.nodes.len() {
                return Err(ValidationError(format!(
                    "node %{0} references invalid node {input}",
                    node.name
                )));
            }
            if input == id {
                return Err(ValidationError(format!(
                    "node %{} directly depends on itself",
                    node.name
                )));
            }
        }
    }

    let mut state = vec![0_u8; graph.nodes.len()];
    fn visit(id: NodeId, graph: &Graph, state: &mut [u8]) -> Result<(), ValidationError> {
        match state[id] {
            1 => {
                return Err(ValidationError(format!(
                    "cycle detected at %{}",
                    graph.nodes[id].name
                )));
            }
            2 => return Ok(()),
            _ => {}
        }
        state[id] = 1;
        for dep in graph.nodes[id].op.inputs().into_iter().flatten() {
            visit(dep, graph, state)?;
        }
        state[id] = 2;
        Ok(())
    }

    for id in 0..graph.nodes.len() {
        visit(id, graph, &mut state)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::parser;

    use super::*;

    #[test]
    fn detects_cycles() {
        let src = "g0 0.1\nadd %a %b %b\nadd %b %a %a\nreturn %a\n";
        let graph = parser::parse(src).unwrap();
        assert!(validate(&graph).is_err());
    }
}
