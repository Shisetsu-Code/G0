use std::collections::BTreeSet;

pub type NodeId = u32;
pub type PortId = u16;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SemanticType {
    Bool,
    Integer(IntegerType),
    BigInteger,
    Rational,
    Decimal(DecimalType),
    Float(FloatType),
    BigFloat(BigFloatType),
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
    Unique(Box<SemanticType>),
    Borrow(Box<SemanticType>),
    Shared(Box<SemanticType>),
    State(Box<SemanticType>),
    Atomic(Box<SemanticType>),
    Versioned(Box<SemanticType>),
}

pub fn type_assignable(
    source: &SemanticType,
    target: &SemanticType,
) -> bool {
    if source == target {
        return true;
    }

    match (source, target) {
        (SemanticType::Integer(source), SemanticType::Integer(target)) => {
            target.contains(source)
        }
        (
            SemanticType::Array(source, source_len),
            SemanticType::Array(target, target_len),
        ) => {
            source_len == target_len
                && type_assignable(source, target)
        }
        (SemanticType::Slice(source), SemanticType::Slice(target))
        | (SemanticType::Option(source), SemanticType::Option(target))
        | (SemanticType::Unique(source), SemanticType::Unique(target))
        | (SemanticType::Borrow(source), SemanticType::Borrow(target))
        | (SemanticType::Shared(source), SemanticType::Shared(target))
        | (SemanticType::State(source), SemanticType::State(target))
        | (SemanticType::Atomic(source), SemanticType::Atomic(target))
        | (SemanticType::Versioned(source), SemanticType::Versioned(target))
        | (SemanticType::Secret(source), SemanticType::Secret(target))
        | (
            SemanticType::Credential(source),
            SemanticType::Credential(target),
        ) => type_assignable(source, target),
        (
            SemanticType::Vector(source, source_len),
            SemanticType::Vector(target, target_len),
        ) => {
            source_len == target_len
                && type_assignable(source, target)
        }
        (
            SemanticType::Result(source_ok, source_err),
            SemanticType::Result(target_ok, target_err),
        ) => {
            type_assignable(source_ok, target_ok)
                && type_assignable(source_err, target_err)
        }
        _ => false,
    }
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
        Ok(Self {
            min: 0,
            max: max as i128,
        })
    }

    pub fn contains(&self, other: &Self) -> bool {
        self.min <= other.min && self.max >= other.max
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DecimalType {
    pub precision_digits: u32,
    pub scale: i32,
}

impl DecimalType {
    pub fn new(
        precision_digits: u32,
        scale: i32,
    ) -> Result<Self, &'static str> {
        if precision_digits == 0 {
            return Err("decimal precision must be greater than zero");
        }
        Ok(Self {
            precision_digits,
            scale,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BigFloatType {
    pub precision_bits: u32,
}

impl BigFloatType {
    pub fn new(precision_bits: u32) -> Result<Self, &'static str> {
        if precision_bits < 2 {
            return Err("big-float precision must be at least 2 bits");
        }
        Ok(Self { precision_bits })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct FloatType {
    /// Required maximum relative error in parts per billion.
    /// Physical representation selection is a backend concern.
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
    LocalExecution,
    RemoteExecution,
    Accelerator,
    Audit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CapabilityClass {
    /// Application/resource authorization such as Read<Message> in a scope.
    Resource,
    Storage,
    Network,
    Clock,
    Entropy,
    Device,
    Process,
    LocalExecution,
    RemoteExecution,
    Accelerator,
    Audit,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Capability {
    pub class: CapabilityClass,
    pub action: String,
    pub resource: String,
    pub scope: String,
}

impl Capability {
    pub fn new(
        class: CapabilityClass,
        action: impl Into<String>,
        resource: impl Into<String>,
        scope: impl Into<String>,
    ) -> Self {
        Self {
            class,
            action: action.into(),
            resource: resource.into(),
            scope: scope.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityMode {
    DefaultDeny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
pub enum Literal {
    Bool(bool),
    Integer(i128),
    Text(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchArm {
    pub tag: String,
    pub graph: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Const(Literal),
    Add,
    Sub,
    Mul,
    Select {
        when_true: String,
        when_false: String,
    },
    Match {
        arms: Vec<MatchArm>,
        default: String,
    },
    Loop {
        condition: String,
        body: String,
        max_iterations: u64,
    },
    Subgraph(String),
    Import(String),
    Instantiate(String),
    StoreRead {
        resource: String,
        fields: Vec<String>,
    },
    StoreCreate {
        resource: String,
        fields: Vec<String>,
    },
    StoreUpdate {
        resource: String,
        fields: Vec<String>,
    },
    StoreDelete {
        resource: String,
    },
    StoreEnumerate {
        resource: String,
    },
    LocalExecute(String),
    RemoteExecute {
        target: String,
        artifact: String,
    },
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceEndpoint {
    GraphInput(PortId),
    NodeOutput { node: NodeId, port: PortId },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum TargetEndpoint {
    NodeInput { node: NodeId, port: PortId },
    GraphOutput(PortId),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    pub from: SourceEndpoint,
    pub to: TargetEndpoint,
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
    fn narrower_integer_range_is_assignable_to_wider_contract() {
        let narrow = SemanticType::Integer(
            IntegerType::new(20, 20).unwrap(),
        );
        let wide = SemanticType::Integer(
            IntegerType::new(0, 100).unwrap(),
        );

        assert!(type_assignable(&narrow, &wide));
        assert!(!type_assignable(&wide, &narrow));
    }

    #[test]
    fn integer_range_containment_is_semantic() {
        let wide = IntegerType::new(0, 1000).unwrap();
        let narrow = IntegerType::new(10, 20).unwrap();
        assert!(wide.contains(&narrow));
        assert!(!narrow.contains(&wide));
    }

    #[test]
    fn arbitrary_precision_numeric_types_are_semantic_types() {
        let decimal = SemanticType::Decimal(
            DecimalType::new(50, 8).unwrap(),
        );
        let big_float = SemanticType::BigFloat(
            BigFloatType::new(1024).unwrap(),
        );

        assert!(matches!(decimal, SemanticType::Decimal(_)));
        assert!(matches!(big_float, SemanticType::BigFloat(_)));
        assert!(matches!(
            SemanticType::BigInteger,
            SemanticType::BigInteger
        ));
        assert!(matches!(
            SemanticType::Rational,
            SemanticType::Rational
        ));
    }

    #[test]
    fn sensitive_types_are_structural_types() {
        let password = SemanticType::Credential(Box::new(SemanticType::Text));
        assert!(matches!(password, SemanticType::Credential(_)));
    }
}
