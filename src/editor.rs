//! Native graph editing model. UI edits GIR directly; invalid intermediate
//! graphs are diagnosable but cannot be saved or executed as valid artifacts.
use crate::{
    execution::{Cancellation, ExecutionObserver, Executor, RuntimeError, TraceEvent},
    gir::*,
    program::ProgramContract,
    program_binary::{ProgramDocument, decode_program, encode_program},
    value::Value,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};
#[path = "editor_forms.rs"]
mod forms;
pub use forms::{OPERATION_NAMES, parse_type};

#[derive(Clone, Copy, PartialEq)]
enum DebugMode {
    Running,
    Step,
    Paused,
}
struct DebugState {
    mode: DebugMode,
    location: Option<(String, NodeId)>,
    breakpoints: BTreeSet<(String, NodeId)>,
    trace: Vec<TraceEvent>,
    stopped: bool,
    done: bool,
    cancellation: Option<Cancellation>,
}
pub struct DebugControl {
    state: Mutex<DebugState>,
    wake: Condvar,
}
impl DebugControl {
    fn new() -> Self {
        Self {
            state: Mutex::new(DebugState {
                mode: DebugMode::Step,
                location: None,
                breakpoints: BTreeSet::new(),
                trace: vec![],
                stopped: false,
                done: false,
                cancellation: None,
            }),
            wake: Condvar::new(),
        }
    }
    pub fn location(&self) -> Option<(String, NodeId)> {
        self.state.lock().unwrap().location.clone()
    }
    pub fn trace(&self) -> Vec<TraceEvent> {
        self.state.lock().unwrap().trace.clone()
    }
    pub fn wait_paused(&self, timeout: Duration) -> Option<(String, NodeId)> {
        let deadline = Instant::now() + timeout;
        let mut state = self.state.lock().unwrap();
        while state.location.is_none() && !state.done && !state.stopped {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            state = self.wake.wait_timeout(state, remaining).unwrap().0;
        }
        state.location.clone()
    }
    pub fn step(&self) {
        self.resume(DebugMode::Step);
    }
    pub fn continue_run(&self) {
        self.resume(DebugMode::Running);
    }
    fn resume(&self, mode: DebugMode) {
        let mut state = self.state.lock().unwrap();
        state.mode = mode;
        state.location = None;
        self.wake.notify_all();
    }
    pub fn pause(&self) {
        let mut state = self.state.lock().unwrap();
        if state.mode == DebugMode::Running {
            state.mode = DebugMode::Step;
        }
    }
    pub fn stop(&self) {
        let mut state = self.state.lock().unwrap();
        state.stopped = true;
        state.location = None;
        if let Some(cancellation) = &state.cancellation {
            cancellation.cancel();
        }
        self.wake.notify_all();
    }
    pub fn toggle_breakpoint(&self, graph: &str, node: NodeId) -> Result<bool, EditorError> {
        if graph.len() > 1024 {
            return Err(EditorError::Limit);
        }
        let mut state = self.state.lock().unwrap();
        let key = (graph.to_owned(), node);
        if state.breakpoints.remove(&key) {
            return Ok(false);
        }
        if state.breakpoints.len() >= 4096 {
            return Err(EditorError::Limit);
        }
        state.breakpoints.insert(key);
        Ok(true)
    }
}
impl ExecutionObserver for DebugControl {
    fn before_node(
        &self,
        graph: &str,
        node: NodeId,
        cancellation: &Cancellation,
    ) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().unwrap();
        if state.stopped || cancellation.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        if state.mode == DebugMode::Step || state.breakpoints.contains(&(graph.into(), node)) {
            state.mode = DebugMode::Paused;
            state.location = Some((graph.into(), node));
            self.wake.notify_all();
            while state.mode == DebugMode::Paused && !state.stopped && !cancellation.is_cancelled()
            {
                state = self
                    .wake
                    .wait_timeout(state, Duration::from_millis(50))
                    .unwrap()
                    .0;
            }
        }
        if state.stopped || cancellation.is_cancelled() {
            Err(RuntimeError::Cancelled)
        } else {
            Ok(())
        }
    }
    fn after_node(&self, event: &TraceEvent) -> Result<(), RuntimeError> {
        let mut state = self.state.lock().unwrap();
        if state.trace.len() >= 10_000 {
            return Err(RuntimeError::TraceLimit);
        }
        state.trace.push(event.clone());
        Ok(())
    }
}
pub struct DebugSession {
    control: Arc<DebugControl>,
    result: mpsc::Receiver<Result<EditorRun, EditorError>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl DebugSession {
    pub fn control(&self) -> Arc<DebugControl> {
        self.control.clone()
    }
    pub fn try_result(&self) -> Option<Result<EditorRun, EditorError>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(EditorError::History)),
        }
    }
    pub fn finish(mut self) -> Result<EditorRun, EditorError> {
        let result = self.result.recv().map_err(|_| EditorError::History)?;
        if let Some(worker) = self.worker.take() {
            worker.join().map_err(|_| EditorError::History)?;
        }
        result
    }
}
impl Drop for DebugSession {
    fn drop(&mut self) {
        self.control.stop();
        // Host operations may be blocking. Closing the window must never wait on a host.
        self.worker.take();
    }
}

/// Optional UI metadata; never part of GIR and never grants execution authority.
#[derive(Clone, Default, Debug)]
pub struct EditorLayout {
    positions: BTreeMap<(String, NodeId), (i32, i32)>,
}
impl EditorLayout {
    pub fn set(&mut self, graph: &str, node: NodeId, x: i32, y: i32) -> Result<(), EditorError> {
        if graph.is_empty()
            || graph.len() > 1024
            || !(0..=100_000).contains(&x)
            || !(60..=100_000).contains(&y)
            || self.positions.len() >= 4096 && !self.positions.contains_key(&(graph.into(), node))
        {
            return Err(EditorError::Limit);
        }
        self.positions.insert((graph.into(), node), (x, y));
        Ok(())
    }
    pub fn get(&self, graph: &str, node: NodeId) -> Option<(i32, i32)> {
        self.positions.get(&(graph.into(), node)).copied()
    }
    pub fn encode(&self) -> Result<Vec<u8>, EditorError> {
        let mut bytes = b"G0L\0".to_vec();
        bytes.extend_from_slice(&(self.positions.len() as u32).to_le_bytes());
        for ((graph, node), (x, y)) in &self.positions {
            bytes.extend_from_slice(&(graph.len() as u16).to_le_bytes());
            bytes.extend_from_slice(graph.as_bytes());
            bytes.extend_from_slice(&node.to_le_bytes());
            bytes.extend_from_slice(&x.to_le_bytes());
            bytes.extend_from_slice(&y.to_le_bytes());
        }
        if bytes.len() > 1024 * 1024 {
            return Err(EditorError::Limit);
        }
        Ok(bytes)
    }
    pub fn decode(bytes: &[u8]) -> Result<Self, EditorError> {
        if bytes.len() > 1024 * 1024 || !bytes.starts_with(b"G0L\0") {
            return Err(EditorError::Format);
        }
        let mut cursor = 4;
        fn take<'a>(
            bytes: &'a [u8],
            cursor: &mut usize,
            len: usize,
        ) -> Result<&'a [u8], EditorError> {
            let result = bytes
                .get(*cursor..cursor.checked_add(len).ok_or(EditorError::Format)?)
                .ok_or(EditorError::Format)?;
            *cursor += len;
            Ok(result)
        }
        let count = u32::from_le_bytes(take(bytes, &mut cursor, 4)?.try_into().unwrap());
        if count > 4096 {
            return Err(EditorError::Limit);
        }
        let mut layout = Self::default();
        for _ in 0..count {
            let len = u16::from_le_bytes(take(bytes, &mut cursor, 2)?.try_into().unwrap()) as usize;
            if len > 1024 {
                return Err(EditorError::Limit);
            }
            let graph = std::str::from_utf8(take(bytes, &mut cursor, len)?)
                .map_err(|_| EditorError::Format)?;
            let node = u32::from_le_bytes(take(bytes, &mut cursor, 4)?.try_into().unwrap());
            let x = i32::from_le_bytes(take(bytes, &mut cursor, 4)?.try_into().unwrap());
            let y = i32::from_le_bytes(take(bytes, &mut cursor, 4)?.try_into().unwrap());
            if layout.get(graph, node).is_some() {
                return Err(EditorError::Format);
            }
            layout.set(graph, node, x, y)?;
        }
        if cursor != bytes.len() {
            return Err(EditorError::Format);
        }
        Ok(layout)
    }
}

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
#[derive(Clone)]
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
    pub fn add_graph(&mut self, name: &str) -> Result<(), EditorError> {
        if name.is_empty()
            || name.len() > 1024
            || self.graph_names().contains(&name)
            || self.graph_names().len() >= 256
        {
            return Err(EditorError::Limit);
        }
        let mut graph = Self::new().graph().clone();
        graph.name = name.into();
        self.before_edit();
        match &mut self.document {
            Document::Program(p) => p.graphs.push(graph),
            Document::Graph(g) => {
                self.document = Document::Program(ProgramDocument {
                    entry_graph: g.name.clone(),
                    graphs: vec![g.clone(), graph],
                    schemas: vec![],
                })
            }
        }
        Ok(())
    }
    pub fn apply_node_form(&mut self, text: &str) -> Result<NodeId, EditorError> {
        self.add_typed_node(forms::parse_node_form(text)?)
    }
    pub fn node_form_text(&self, id: NodeId) -> Result<String, EditorError> {
        forms::node_form_text(
            self.graph()
                .nodes
                .iter()
                .find(|n| n.id == id)
                .ok_or(EditorError::Node)?,
        )
    }
    pub fn apply_node_form_to(&mut self, id: NodeId, text: &str) -> Result<(), EditorError> {
        if !self.graph().nodes.iter().any(|n| n.id == id) {
            return Err(EditorError::Node);
        }
        let mut node = forms::parse_node_form(text)?;
        node.id = id;
        validate_typed_node(&node)?;
        self.before_edit();
        *self
            .graph_mut()
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .unwrap() = node;
        Ok(())
    }
    pub fn apply_graph_form(&mut self, text: &str) -> Result<(), EditorError> {
        let (inputs, outputs) = forms::parse_graph_form(text)?;
        let mut probe = Graph::new("editor-interface");
        probe.inputs = inputs.clone();
        probe.outputs = outputs.clone();
        // Duplicate names and port IDs are structural errors. Output drivers
        // belong to the edited graph and are checked before save/run.
        if let Err(report) = crate::gir_validate::validate(&probe) {
            let issues: Vec<_> = report
                .issues
                .into_iter()
                .filter(|i| i.code != crate::gir_validate::ValidationCode::MissingGraphOutputDriver)
                .collect();
            if !issues.is_empty() {
                return Err(EditorError::Validation(format!("{issues:?}")));
            }
        }
        self.before_edit();
        self.graph_mut().inputs = inputs;
        self.graph_mut().outputs = outputs;
        Ok(())
    }
    pub fn graph_form_text(&self) -> String {
        match (
            forms::ports_text(&self.graph().inputs),
            forms::ports_text(&self.graph().outputs),
        ) {
            (Ok(inputs), Ok(outputs)) => format!("inputs={inputs}\r\noutputs={outputs}"),
            _ => "Interface names contain unsupported delimiters; use native API.".into(),
        }
    }
    pub fn apply_schema_form(&mut self, text: &str) -> Result<(), EditorError> {
        let schema = forms::parse_schema_form(text)?;
        if let Document::Program(p) = &self.document
            && p.schemas.len() >= 256
            && !p.schemas.iter().any(|s| s.name == schema.name)
        {
            return Err(EditorError::Limit);
        }
        self.before_edit();
        if let Document::Graph(graph) = &self.document {
            self.document = Document::Program(ProgramDocument {
                entry_graph: graph.name.clone(),
                graphs: vec![graph.clone()],
                schemas: vec![],
            });
        }
        if let Document::Program(p) = &mut self.document {
            if let Some(existing) = p.schemas.iter_mut().find(|s| s.name == schema.name) {
                *existing = schema;
            } else {
                p.schemas.push(schema);
            }
        }
        Ok(())
    }
    pub fn schema_properties(&self) -> String {
        match &self.document {
            Document::Program(p) => format!("{:#?}", p.schemas),
            _ => "[]".into(),
        }
    }
    pub fn node_properties(&self, id: NodeId) -> Result<String, EditorError> {
        let node = self
            .graph()
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or(EditorError::Node)?;
        Ok(format!(
            "{} / nodo {}: {:?}\r\nEntradas: {:?}\r\nSalidas: {:?}\r\nEfectos: {:?}\r\nCapacidades requeridas: {:?}",
            self.graph().name,
            id,
            node.operation,
            node.inputs,
            node.outputs,
            node.effects,
            node.required_capabilities
        ))
    }
    pub fn set_port_type(
        &mut self,
        node: Option<NodeId>,
        output: bool,
        port: PortId,
        ty: SemanticType,
    ) -> Result<(), EditorError> {
        if matches!(&ty, SemanticType::Integer(t) if t.min > t.max) {
            return Err(EditorError::TypeMismatch);
        }
        let ports = match node {
            Some(id) => {
                let n = self
                    .graph()
                    .nodes
                    .iter()
                    .find(|n| n.id == id)
                    .ok_or(EditorError::Node)?;
                if output { &n.outputs } else { &n.inputs }
            }
            None => {
                if output {
                    &self.graph().outputs
                } else {
                    &self.graph().inputs
                }
            }
        };
        if !ports.iter().any(|p| p.id == port) {
            return Err(EditorError::Endpoint);
        }
        self.before_edit();
        let ports = match node {
            Some(id) => {
                let n = self
                    .graph_mut()
                    .nodes
                    .iter_mut()
                    .find(|n| n.id == id)
                    .unwrap();
                if output {
                    &mut n.outputs
                } else {
                    &mut n.inputs
                }
            }
            None => {
                if output {
                    &mut self.graph_mut().outputs
                } else {
                    &mut self.graph_mut().inputs
                }
            }
        };
        ports.iter_mut().find(|p| p.id == port).unwrap().ty = ty;
        Ok(())
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
        let total_nodes: usize = match &self.document {
            Document::Graph(g) => g.nodes.len(),
            Document::Program(p) => p.graphs.iter().map(|g| g.nodes.len()).sum(),
        };
        if total_nodes >= 4096 {
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
    pub fn add_literal(&mut self, literal: Literal) -> Result<NodeId, EditorError> {
        let ty = literal_type(&literal)?;
        let id = self.next_id()?;
        self.before_edit();
        self.graph_mut().nodes.push(Node {
            id,
            operation: Operation::Const(literal),
            inputs: vec![],
            outputs: vec![Port {
                id: 0,
                name: "value".into(),
                ty,
            }],
            effects: Default::default(),
            required_capabilities: Default::default(),
        });
        Ok(id)
    }
    /// General typed form for every GIR operation. References and resource
    /// contracts are validated at the program boundary before save/run.
    pub fn add_typed_node(&mut self, mut node: Node) -> Result<NodeId, EditorError> {
        let id = self.next_id()?;
        node.id = id;
        validate_typed_node(&node)?;
        self.before_edit();
        self.graph_mut().nodes.push(node);
        Ok(id)
    }
    pub fn duplicate_node(&mut self, id: NodeId) -> Result<NodeId, EditorError> {
        let node = self
            .graph()
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or(EditorError::Node)?
            .clone();
        self.add_typed_node(node)
    }
    pub fn add_subgraph(&mut self, name: &str) -> Result<NodeId, EditorError> {
        let graph = match &self.document {
            Document::Graph(g) if g.name == name => Some(g),
            Document::Program(p) => p.graphs.iter().find(|g| g.name == name),
            _ => None,
        }
        .ok_or(EditorError::Node)?;
        self.add_typed_node(Node {
            id: 0,
            operation: Operation::Subgraph(name.into()),
            inputs: graph.inputs.clone(),
            outputs: graph.outputs.clone(),
            effects: Default::default(),
            required_capabilities: Default::default(),
        })
    }
    pub fn set_output_type(&mut self, port: PortId, ty: SemanticType) -> Result<(), EditorError> {
        self.set_port_type(None, true, port, ty)
    }
    /// Parse according to the existing literal's type, rather than silently changing types.
    pub fn set_literal_text(&mut self, id: NodeId, text: &str) -> Result<(), EditorError> {
        let node = self
            .graph()
            .nodes
            .iter()
            .find(|n| n.id == id)
            .ok_or(EditorError::Node)?;
        let literal = match &node.operation {
            Operation::Const(Literal::Integer(_)) => {
                Literal::Integer(text.trim().parse().map_err(|_| EditorError::Format)?)
            }
            Operation::Const(Literal::Bool(_)) => {
                Literal::Bool(text.trim().parse().map_err(|_| EditorError::Format)?)
            }
            Operation::Const(Literal::Text(_)) => Literal::Text(text.into()),
            Operation::Const(Literal::Bytes(_)) => Literal::Bytes(parse_bytes(text)?),
            _ => return Err(EditorError::Node),
        };
        let ty = literal_type(&literal)?;
        if node.outputs.len() != 1 {
            return Err(EditorError::Node);
        }
        self.before_edit();
        let node = self
            .graph_mut()
            .nodes
            .iter_mut()
            .find(|n| n.id == id)
            .unwrap();
        node.operation = Operation::Const(literal);
        node.outputs[0].ty = ty;
        Ok(())
    }
    pub fn add_operation(&mut self, operation: Operation) -> Result<NodeId, EditorError> {
        let range = |min, max| SemanticType::Integer(IntegerType { min, max });
        let integer = range(-1_000_000, 1_000_000);
        let (inputs, output) = match operation {
            Operation::Add | Operation::Sub => {
                (vec![integer.clone(); 2], range(-2_000_000, 2_000_000))
            }
            Operation::Mul => (
                vec![integer.clone(); 2],
                range(-1_000_000_000_000, 1_000_000_000_000),
            ),
            Operation::Div | Operation::Rem => {
                (vec![integer.clone(), range(1, 1_000_000)], integer.clone())
            }
            Operation::Eq | Operation::Lt | Operation::Le | Operation::Gt | Operation::Ge => {
                (vec![integer.clone(); 2], SemanticType::Bool)
            }
            Operation::And | Operation::Or | Operation::Xor => {
                (vec![SemanticType::Bool; 2], SemanticType::Bool)
            }
            Operation::Not => (vec![SemanticType::Bool], SemanticType::Bool),
            Operation::TextConcat => (vec![SemanticType::Text; 2], SemanticType::Text),
            Operation::BytesConcat => (vec![SemanticType::Bytes; 2], SemanticType::Bytes),
            Operation::EncodeUtf8 => (vec![SemanticType::Text], SemanticType::Bytes),
            Operation::DecodeInteger128Le => (vec![SemanticType::Bytes], range(i128::MIN, i128::MAX)),
            Operation::DecodeUtf8 => (
                vec![SemanticType::Bytes],
                SemanticType::Result(Box::new(SemanticType::Text), Box::new(SemanticType::Bytes)),
            ),
            Operation::FormatInteger => (vec![integer], SemanticType::Text),
            Operation::ConvertChecked => (
                vec![range(-1_000_000, 1_000_000)],
                range(-1_000_000, 1_000_000),
            ),
            Operation::MakeArray => (
                vec![integer.clone(); 2],
                SemanticType::Array(Box::new(integer), 2),
            ),
            Operation::Index => (
                vec![SemanticType::Bytes, range(0, u64::MAX as i128)],
                SemanticType::Option(Box::new(range(0, 255))),
            ),
            Operation::Length => (vec![SemanticType::Bytes], range(0, u64::MAX as i128)),
            Operation::Some => (
                vec![integer.clone()],
                SemanticType::Option(Box::new(integer)),
            ),
            Operation::None => (vec![], SemanticType::Option(Box::new(integer))),
            Operation::Ok => (
                vec![integer.clone()],
                SemanticType::Result(Box::new(integer), Box::new(SemanticType::Text)),
            ),
            Operation::Err => (
                vec![SemanticType::Text],
                SemanticType::Result(Box::new(integer), Box::new(SemanticType::Text)),
            ),
            Operation::UnwrapOr => (
                vec![
                    SemanticType::Option(Box::new(integer.clone())),
                    integer.clone(),
                ],
                integer,
            ),
            Operation::Range => (
                vec![range(0, 1_000_000)],
                SemanticType::Slice(Box::new(range(0, 999_999))),
            ),
            Operation::ArrayConcat => (
                vec![SemanticType::Slice(Box::new(integer.clone())); 2],
                SemanticType::Slice(Box::new(integer)),
            ),
            Operation::BytesSlice => (
                vec![
                    SemanticType::Bytes,
                    range(0, u64::MAX as i128),
                    range(0, u64::MAX as i128),
                ],
                SemanticType::Bytes,
            ),
            Operation::BytesFromArray => (
                vec![SemanticType::Slice(Box::new(range(0, 255)))],
                SemanticType::Bytes,
            ),
            Operation::TextJoin => (
                vec![
                    SemanticType::Slice(Box::new(SemanticType::Text)),
                    SemanticType::Text,
                ],
                SemanticType::Text,
            ),
            _ => return Err(EditorError::Node),
        };
        let id = self.next_id()?;
        let node = Node {
            id,
            operation,
            inputs: inputs
                .into_iter()
                .enumerate()
                .map(|(id, ty)| Port {
                    id: id as PortId,
                    name: format!("arg{id}"),
                    ty,
                })
                .collect(),
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
        let bytes = match &self.document {
            Document::Graph(g) => {
                crate::graph_binary::encode_graph(g).map_err(|_| EditorError::Format)
            }
            Document::Program(p) => encode_program(p).map_err(|_| EditorError::Format),
        }?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(EditorError::Limit);
        }
        Ok(bytes)
    }
    pub fn run(&self) -> Result<EditorRun, EditorError> {
        self.run_observed(None)
    }
    pub fn start_debugger(&self) -> Result<DebugSession, EditorError> {
        self.validate()?;
        let editor = self.clone();
        let control = Arc::new(DebugControl::new());
        let observed = control.clone();
        let (sender, result) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = editor.run_observed(Some(observed.clone()));
            {
                let mut state = observed.state.lock().unwrap();
                state.done = true;
                state.location = None;
                observed.wake.notify_all();
            }
            let _ = sender.send(result);
        });
        Ok(DebugSession {
            control,
            result,
            worker: Some(worker),
        })
    }
    fn run_observed(&self, observer: Option<Arc<DebugControl>>) -> Result<EditorRun, EditorError> {
        self.validate()?;
        let contract = self.contract()?;
        let mut runtime =
            Executor::new(&contract, Default::default()).map_err(EditorError::Runtime)?;
        let mut resources = crate::resource_host::ResourceHost::new(
            std::sync::Arc::new(contract.clone()),
            Default::default(),
            Default::default(),
        )
        .map_err(EditorError::Runtime)?;
        runtime.set_cancellation(resources.cancellation());
        if let Some(observer) = observer {
            {
                let mut state = observer.state.lock().unwrap();
                let cancellation = resources.cancellation();
                if state.stopped {
                    cancellation.cancel();
                }
                state.cancellation = Some(cancellation);
            }
            runtime.set_observer(observer);
        }
        runtime.enable_trace(10_000).map_err(EditorError::Runtime)?;
        let values = runtime
            .run_with_host(&self.graph().name, vec![], &mut resources)
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
fn validate_typed_node(node: &Node) -> Result<(), EditorError> {
    if node.inputs.len() > 64
        || node.outputs.len() > 64
        || node
            .inputs
            .iter()
            .chain(&node.outputs)
            .any(|p| p.name.len() > 1024)
    {
        return Err(EditorError::Limit);
    }
    let mut probe = Graph::new("editor-form");
    probe.inputs = node.inputs.clone();
    probe.outputs = node.outputs.clone();
    for p in &node.inputs {
        probe.edges.push(Edge {
            from: SourceEndpoint::GraphInput(p.id),
            to: TargetEndpoint::NodeInput {
                node: node.id,
                port: p.id,
            },
        });
    }
    for p in &node.outputs {
        probe.edges.push(Edge {
            from: SourceEndpoint::NodeOutput {
                node: node.id,
                port: p.id,
            },
            to: TargetEndpoint::GraphOutput(p.id),
        });
    }
    probe.nodes.push(node.clone());
    crate::gir_validate::validate(&probe).map_err(|e| EditorError::Validation(e.to_string()))
}

fn literal_type(literal: &Literal) -> Result<SemanticType, EditorError> {
    Ok(match literal {
        Literal::Integer(value) => SemanticType::Integer(IntegerType {
            min: *value,
            max: *value,
        }),
        Literal::Bool(_) => SemanticType::Bool,
        Literal::Text(text) if text.len() <= 64 * 1024 => SemanticType::Text,
        Literal::Bytes(bytes) if bytes.len() <= 64 * 1024 => SemanticType::Bytes,
        _ => return Err(EditorError::Limit),
    })
}
fn parse_bytes(text: &str) -> Result<Vec<u8>, EditorError> {
    if text.len() > 128 * 1024 {
        return Err(EditorError::Limit);
    }
    if !text.is_ascii() || !text.len().is_multiple_of(2) {
        return Err(EditorError::Format);
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| EditorError::Format))
        .collect()
}
