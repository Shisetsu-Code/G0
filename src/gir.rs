use std::collections::BTreeSet;

pub type NodeId = u32;
pub type PortId = u16;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SemanticType {
    Bool,
    Integer(IntegerType),
    Float(FloatType),
    Text,
    Bytes,
    Array(Box<SemanticType>, usize),
    Slice(Box<SemanticType>),
    Vector(Box<SemanticType>, usize),
    Record(String),
    Variant(String),
    Option(Box<SemanticType>),
    Result(Box<SemanticType>, Box<SemanticType>),
    Reference(String),
    Secret(Box<SemanticType>),
    Credential(Box<SemanticType>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct IntegerType {
    pub min: i128,
    pub max: i128,
}

impl IntegerType {
    pub fn new(min: i128, max: i128) -> Result<Self, &'static str> {
        if min > max {
            return Err("integer semantic range has min > max");
        }
        Ok(Self { min, max })
    }

    pub fn unsigned(max: u128) -> Result<Self, &'static str> {
        if max > i128::MAX as u128 {
            return Err("bootstrap GIR cannot yet represent unsigned ranges above i128::MAX");
        }
        Ok(Self { min: 0, max: max as i128 })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FloatType {
    /// Required maximum relative error in parts per billion.
    /// Representation selection is a backend concern.
    pub max_relative_error_ppb: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effect {
    MemoryWrite,
    Storage,
    Network,
    Clock,
    Entropy,
    Device,
    Process,
    RemoteExecution,
    Accelerator,
    Audit,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Capability {
    pub action: String,
    pub resource: String,
    pub scope: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityMode {
    DefaultDeny,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParameterPolicy<T> {
    Fixed(T),
    Bounded { min: T, max: T },
    Free,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    pub id: PortId,
    pub name: String,
    pub ty: SemanticType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Const,
    Add,
    Sub,
    Mul,
    Select,
    Match,
    Loop,
    Subgraph(String),
    Import(String),
    Instantiate(String),
    Execute(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: NodeId,
    pub operation: Operation,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    pub effects: BTreeSet<Effect>,
    pub required_capabilities: BTreeSet<Capability>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from_node: NodeId,
    pub from_port: PortId,
    pub to_node: NodeId,
    pub to_port: PortId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Graph {
    pub name: String,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub authority: AuthorityMode,
}

impl Graph {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            nodes: Vec::new(),
            edges: Vec::new(),
            authority: AuthorityMode::DefaultDeny,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_authority_is_deny() {
        let graph = Graph::new("test");
        assert_eq!(graph.authority, AuthorityMode::DefaultDeny);
    }

    #[test]
    fn integer_ranges_reject_invalid_bounds() {
        assert!(IntegerType::new(10, 9).is_err());
    }

    #[test]
    fn sensitive_types_are_structural_types() {
        let password = SemanticType::Credential(Box::new(SemanticType::Text));
        assert!(matches!(password, SemanticType::Credential(_)));
    }
}
