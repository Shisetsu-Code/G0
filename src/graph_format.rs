use crate::gir::{Edge, Graph, Port, SourceEndpoint, TargetEndpoint};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphFormatVersion {
    pub major: u16,
    pub minor: u16,
}

impl GraphFormatVersion {
    pub const BOOTSTRAP: Self = Self { major: 0, minor: 3 };
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalGraphDocument {
    pub version: GraphFormatVersion,
    pub graph: Graph,
}

pub fn canonicalize_graph(graph: &Graph) -> CanonicalGraphDocument {
    let mut graph = graph.clone();

    graph.inputs.sort_by_key(port_key);
    graph.outputs.sort_by_key(port_key);

    for node in &mut graph.nodes {
        node.inputs.sort_by_key(port_key);
        node.outputs.sort_by_key(port_key);
    }

    graph.nodes.sort_by_key(|node| node.id);
    graph.edges.sort_by_key(edge_key);

    CanonicalGraphDocument {
        version: GraphFormatVersion::BOOTSTRAP,
        graph,
    }
}

fn port_key(port: &Port) -> (u16, String) {
    (port.id, port.name.clone())
}

fn edge_key(edge: &Edge) -> (u8, u32, u16, u8, u32, u16) {
    let source = match edge.from {
        SourceEndpoint::GraphInput(port) => (0, 0, port),
        SourceEndpoint::NodeOutput { node, port } => (1, node, port),
    };
    let target = match edge.to {
        TargetEndpoint::NodeInput { node, port } => (0, node, port),
        TargetEndpoint::GraphOutput(port) => (1, 0, port),
    };

    (source.0, source.1, source.2, target.0, target.1, target.2)
}

pub fn structurally_equal(a: &Graph, b: &Graph) -> bool {
    canonicalize_graph(a) == canonicalize_graph(b)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalGraphIssue {
    InvalidGraph,
}

pub fn validated_canonical_graph(
    graph: &Graph,
) -> Result<CanonicalGraphDocument, CanonicalGraphIssue> {
    crate::gir_validate::validate(graph).map_err(|_| CanonicalGraphIssue::InvalidGraph)?;
    Ok(canonicalize_graph(graph))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::gir::{AuthorityMode, IntegerType, Literal, Node, Operation, SemanticType};

    fn node(id: u32, value: i128) -> Node {
        Node {
            id,
            operation: Operation::Const(Literal::Integer(value)),
            inputs: vec![],
            outputs: vec![Port {
                id: 0,
                name: "out".into(),
                ty: SemanticType::Integer(IntegerType::new(value, value).unwrap()),
            }],
            effects: BTreeSet::new(),
            required_capabilities: BTreeSet::new(),
        }
    }

    #[test]
    fn textual_node_order_does_not_change_canonical_graph() {
        let a = Graph {
            name: "g".into(),
            inputs: vec![],
            outputs: vec![],
            nodes: vec![node(2, 2), node(1, 1)],
            edges: vec![],
            authority: AuthorityMode::DefaultDeny,
        };
        let b = Graph {
            nodes: vec![node(1, 1), node(2, 2)],
            ..a.clone()
        };

        assert!(structurally_equal(&a, &b));
        assert_eq!(
            canonicalize_graph(&a)
                .graph
                .nodes
                .iter()
                .map(|node| node.id)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
    }
}
