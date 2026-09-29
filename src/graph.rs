use std::collections::BTreeMap;

pub type NodeId = usize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Type {
    I64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Const(i64),
    Add(NodeId, NodeId),
    Sub(NodeId, NodeId),
    Mul(NodeId, NodeId),
}

impl Op {
    pub fn inputs(&self) -> [Option<NodeId>; 2] {
        match *self {
            Self::Const(_) => [None, None],
            Self::Add(a, b) | Self::Sub(a, b) | Self::Mul(a, b) => [Some(a), Some(b)],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub name: String,
    pub ty: Type,
    pub op: Op,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Graph {
    pub version: String,
    pub target: String,
    pub nodes: Vec<Node>,
    pub output: NodeId,
    pub names: BTreeMap<String, NodeId>,
}

impl Graph {
    pub fn rebuild_names(&mut self) {
        self.names.clear();
        for (id, node) in self.nodes.iter().enumerate() {
            self.names.insert(node.name.clone(), id);
        }
    }
}
