//! Native graph editing model. UI edits GIR directly; invalid intermediate
//! graphs are diagnosable but cannot be saved or executed as valid artifacts.
use crate::{
    execution::{Executor, RuntimeError, TraceEvent},
    gir::*,
    program::ProgramContract,
    program_binary::{ProgramDocument, decode_program, encode_program},
    value::Value,
};
use std::collections::BTreeSet;

#[derive(Debug)]
pub enum EditorError {
    Format,
    Validation(String),
    Endpoint,
    TypeMismatch,
    Cycle,
    Node,
    Limit,
    History,
    Runtime(RuntimeError),
}
#[derive(Clone)]
enum Document {
    Graph(Graph),
    Program(ProgramDocument),
}
pub struct GraphEditor {
    document: Document,
    selected: usize,
    undo: Vec<Document>,
    redo: Vec<Document>,
}
#[derive(Debug)]
pub struct EditorRun {
    pub values: Vec<Value>,
    pub trace: Vec<TraceEvent>,
    pub steps: u64,
}
impl Default for GraphEditor {
    fn default() -> Self {
        Self::new()
    }
}
impl GraphEditor {
    pub fn new() -> Self {
        let mut graph = Graph::new("main");
        graph.outputs = vec![Port {
            id: 0,
            name: "result".into(),
            ty: SemanticType::Integer(IntegerType {
                min: i64::MIN as i128,
                max: i64::MAX as i128,
            }),
        }];
        graph.nodes.push(integer_node(1, 42));
        graph.edges.push(Edge {
            from: SourceEndpoint::NodeOutput { node: 1, port: 0 },
            to: TargetEndpoint::GraphOutput(0),
        });
        Self {
            document: Document::Graph(graph),
            selected: 0,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, EditorError> {
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(EditorError::Limit);
        }
        let document = if bytes.starts_with(b"G0P\0") {
            Document::Program(decode_program(bytes).map_err(|_| EditorError::Format)?)
        } else {
            Document::Graph(
                crate::graph_binary_decode::decode_graph(bytes).map_err(|_| EditorError::Format)?,
            )
        };
        let selected = match &document {
            Document::Program(p) => p
                .graphs
                .iter()
                .position(|g| g.name == p.entry_graph)
                .unwrap_or(0),
            Document::Graph(_) => 0,
        };
        let nodes = match &document {
            Document::Program(p) => p.graphs.iter().map(|g| g.nodes.len()).sum(),
            Document::Graph(g) => g.nodes.len(),
        };
        if nodes > 4096 {
            return Err(EditorError::Limit);
        }
        Ok(Self {
            document,
            selected,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }
    pub fn graph_names(&self) -> Vec<&str> {
        match &self.document {
            Document::Graph(g) => vec![&g.name],
            Document::Program(p) => p.graphs.iter().map(|g| g.name.as_str()).collect(),
        }
    }
    pub fn select_graph(&mut self, index: usize) -> Result<(), EditorError> {
        if index >= self.graph_names().len() {
            return Err(EditorError::Node);
        }
        self.selected = index;
        Ok(())
    }
    pub fn graph(&self) -> &Graph {
        match &self.document {
            Document::Graph(g) => g,
            Document::Program(p) => &p.graphs[self.selected],
        }
    }
    fn graph_mut(&mut self) -> &mut Graph {
        match &mut self.document {
            Document::Graph(g) => g,
            Document::Program(p) => &mut p.graphs[self.selected],
        }
    }
    fn before_edit(&mut self) {
        if self.undo.len() == 32 {
            self.undo.remove(0);
        }
        self.undo.push(self.document.clone());
        self.redo.clear();
    }
    fn next_id(&self) -> Result<NodeId, EditorError> {
        if self.graph().nodes.len() >= 4096 {
            return Err(EditorError::Limit);
        }
        self.graph()
            .nodes
            .iter()
            .map(|n| n.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(EditorError::Limit)
    }
    pub fn add_integer(&mut self, value: i128) -> Result<NodeId, EditorError> {
        let id = self.next_id()?;
        self.before_edit();
        self.graph_mut().nodes.push(integer_node(id, value));
        Ok(id)
    }
    pub fn add_operation(&mut self, operation: Operation) -> Result<NodeId, EditorError> {
        let range = |min, max| SemanticType::Integer(IntegerType { min, max });
        let output = match operation {
            Operation::Add => range(-2_000_000, 2_000_000),
            Operation::Sub => range(-2_000_000, 2_000_000),
            Operation::Mul => range(-1_000_000_000_000, 1_000_000_000_000),
            _ => return Err(EditorError::Node),
        };
        let id = self.next_id()?;
        let node = Node {
            id,
            operation,
            inputs: vec![
                Port {
                    id: 0,
                    name: "a".into(),
                    ty: range(-1_000_000, 1_000_000),
                },
                Port {
                    id: 1,
                    name: "b".into(),
                    ty: range(-1_000_000, 1_000_000),
                },
            ],
            outputs: vec![Port {
                id: 0,
                name: "result".into(),
                ty: output,
            }],
            effects: Default::default(),
            required_capabilities: Default::default(),
        };
        self.before_edit();
        self.graph_mut().nodes.push(node);
        Ok(id)
    }
    pub fn set_integer(&mut self, id: NodeId, value: i128) -> Result<(), EditorError> {
        let node = self
            .graph()
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or(EditorError::Node)?;
        if !matches!(node.operation, Operation::Const(Literal::Integer(_)))
            || node.outputs.len() != 1
        {
            return Err(EditorError::Node);
        }
        self.before_edit();
        let node = self
            .graph_mut()
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .unwrap();
        node.operation = Operation::Const(Literal::Integer(value));
        node.outputs[0].ty = SemanticType::Integer(IntegerType {
            min: value,
            max: value,
        });
        Ok(())
    }
    pub fn delete_node(&mut self, id: NodeId) -> Result<(), EditorError> {
        if !self.graph().nodes.iter().any(|n| n.id == id) {
            return Err(EditorError::Node);
        }
        self.before_edit();
        let graph = self.graph_mut();
        graph.nodes.retain(|n| n.id != id);
        graph.edges.retain(|e| {
            !matches!(e.from,SourceEndpoint::NodeOutput { node,.. } if node == id)
                && !matches!(e.to,TargetEndpoint::NodeInput { node,.. } if node == id)
        });
        Ok(())
    }
    pub fn connect(&mut self, from: SourceEndpoint, to: TargetEndpoint) -> Result<(), EditorError> {
        let graph = self.graph();
        let source = match &from {
            SourceEndpoint::GraphInput(id) => graph.inputs.iter().find(|p| p.id == *id),
            SourceEndpoint::NodeOutput { node, port } => graph
                .nodes
                .iter()
                .find(|n| n.id == *node)
                .and_then(|n| n.outputs.iter().find(|p| p.id == *port)),
        }
        .ok_or(EditorError::Endpoint)?;
        let target = match &to {
            TargetEndpoint::GraphOutput(id) => graph.outputs.iter().find(|p| p.id == *id),
            TargetEndpoint::NodeInput { node, port } => graph
                .nodes
                .iter()
                .find(|n| n.id == *node)
                .and_then(|n| n.inputs.iter().find(|p| p.id == *port)),
        }
        .ok_or(EditorError::Endpoint)?;
        if !type_assignable(&source.ty, &target.ty) {
            return Err(EditorError::TypeMismatch);
        }
        if let (
            SourceEndpoint::NodeOutput { node: source, .. },
            TargetEndpoint::NodeInput { node: target, .. },
        ) = (&from, &to)
        {
            let mut pending = vec![*target];
            let mut visited = BTreeSet::new();
            while let Some(node) = pending.pop() {
                if node == *source {
                    return Err(EditorError::Cycle);
                }
                if visited.insert(node) {
                    for edge in &graph.edges {
                        if let (
                            SourceEndpoint::NodeOutput { node: parent, .. },
                            TargetEndpoint::NodeInput { node: child, .. },
                        ) = (&edge.from, &edge.to)
                            && *parent == node
                        {
                            pending.push(*child);
                        }
                    }
                }
            }
        }
        self.before_edit();
        let graph = self.graph_mut();
        graph.edges.retain(|e| e.to != to);
        graph.edges.push(Edge { from, to });
        Ok(())
    }
    pub fn undo(&mut self) -> Result<(), EditorError> {
        let previous = self.undo.pop().ok_or(EditorError::History)?;
        self.redo
            .push(std::mem::replace(&mut self.document, previous));
        self.selected = self
            .selected
            .min(self.graph_names().len().saturating_sub(1));
        Ok(())
    }
    pub fn redo(&mut self) -> Result<(), EditorError> {
        let next = self.redo.pop().ok_or(EditorError::History)?;
        self.undo.push(std::mem::replace(&mut self.document, next));
        self.selected = self
            .selected
            .min(self.graph_names().len().saturating_sub(1));
        Ok(())
    }
    fn contract(&self) -> Result<ProgramContract, EditorError> {
        match &self.document {
            Document::Graph(graph) => Ok(ProgramContract {
                graphs: vec![graph.clone()],
                entry_graph: Some(graph.name.clone()),
                ..Default::default()
            }),
            Document::Program(document) => document
                .validated_contract()
                .map_err(|e| EditorError::Validation(format!("{e:?}"))),
        }
    }
    pub fn validate(&self) -> Result<(), EditorError> {
        let contract = self.contract()?;
        crate::program::validate_program(
            &contract,
            &crate::program::PlatformContract::bootstrap_x86_64_v3(),
        )
        .map_err(|issues| EditorError::Validation(format!("{issues:?}")))
    }
    pub fn encode(&self) -> Result<Vec<u8>, EditorError> {
        self.validate()?;
        match &self.document {
            Document::Graph(g) => {
                crate::graph_binary::encode_graph(g).map_err(|_| EditorError::Format)
            }
            Document::Program(p) => encode_program(p).map_err(|_| EditorError::Format),
        }
    }
    pub fn run(&self) -> Result<EditorRun, EditorError> {
        self.validate()?;
        let contract = self.contract()?;
        let mut runtime =
            Executor::new(&contract, Default::default()).map_err(EditorError::Runtime)?;
        runtime.enable_trace(10_000).map_err(EditorError::Runtime)?;
        let values = runtime
            .run_graph(&self.graph().name, vec![])
            .map_err(EditorError::Runtime)?;
        Ok(EditorRun {
            values,
            trace: runtime.trace().to_vec(),
            steps: runtime.steps_used(),
        })
    }
}
fn integer_node(id: NodeId, value: i128) -> Node {
    Node {
        id,
        operation: Operation::Const(Literal::Integer(value)),
        inputs: vec![],
        outputs: vec![Port {
            id: 0,
            name: "value".into(),
            ty: SemanticType::Integer(IntegerType {
                min: value,
                max: value,
            }),
        }],
        effects: Default::default(),
        required_capabilities: Default::default(),
    }
}
