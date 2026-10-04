//! Bounded GIR execution on the native bootstrap host. No ambient authority.
use crate::{
    gir::*,
    program::{PlatformContract, ProgramContract, validate_program},
    value::Value,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Debug, Clone, Copy)]
pub struct ExecutionLimits {
    pub max_steps: u64,
    pub max_value_bytes: u64,
    pub max_call_depth: usize,
}
impl Default for ExecutionLimits {
    fn default() -> Self {
        Self {
            max_steps: 1_000_000,
            max_value_bytes: 64 * 1024 * 1024,
            max_call_depth: 128,
        }
    }
}

#[derive(Clone, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeError {
    TraceLimit,
    InvalidProgram,
    MissingGraph(String),
    MissingEntry,
    StepLimit,
    MemoryLimit,
    CallDepth,
    Cancelled,
    TypeMismatch { graph: String, node: Option<NodeId> },
    Arithmetic { graph: String, node: NodeId },
    Bounds { graph: String, node: NodeId },
    LoopLimit { graph: String, node: NodeId },
    MissingCapability(Capability),
    Unsupported { graph: String, node: NodeId },
    InvalidHostResult,
    HostCompletion { graph: String, code: &'static str },
    EffectFailure { node: NodeId, code: &'static str },
}

pub trait EffectHost {
    fn execute(&mut self, node: &Node, inputs: &[Value]) -> Result<Vec<Value>, RuntimeError>;
}
struct DenyHost;
impl EffectHost for DenyHost {
    fn execute(&mut self, _: &Node, _: &[Value]) -> Result<Vec<Value>, RuntimeError> {
        Err(RuntimeError::InvalidHostResult)
    }
}

pub struct Executor<'a> {
    program: &'a ProgramContract,
    limits: ExecutionLimits,
    steps: u64,
    bytes: u64,
    cancelled: Cancellation,
    capabilities: BTreeSet<Capability>,
    trace: Option<(usize, Vec<TraceEvent>)>,
    observer: Option<Arc<dyn ExecutionObserver>>,
}

#[derive(Debug, Clone)]
pub struct TraceEvent {
    pub graph: String,
    pub node: NodeId,
    pub outputs: Vec<Value>,
}

/// Trusted debugger binding. Blocking callbacks preserve the worker's call stack.
/// A paused observer must wake on cancellation; callbacks cannot grant authority.
pub trait ExecutionObserver: Send + Sync {
    fn before_node(
        &self,
        graph: &str,
        node: NodeId,
        cancellation: &Cancellation,
    ) -> Result<(), RuntimeError>;
    fn after_node(&self, _event: &TraceEvent) -> Result<(), RuntimeError> {
        Ok(())
    }
}
impl<'a> Executor<'a> {
    pub fn new(
        program: &'a ProgramContract,
        limits: ExecutionLimits,
    ) -> Result<Self, RuntimeError> {
        validate_program(program, &PlatformContract::bootstrap_x86_64_v3())
            .map_err(|_| RuntimeError::InvalidProgram)?;
        Ok(Self {
            program,
            limits,
            steps: 0,
            bytes: 0,
            cancelled: Cancellation::default(),
            capabilities: BTreeSet::new(),
            trace: None,
            observer: None,
        })
    }
    pub fn cancellation(&self) -> Cancellation {
        self.cancelled.clone()
    }
    pub fn set_cancellation(&mut self, cancellation: Cancellation) {
        self.cancelled = cancellation;
    }
    pub fn set_observer(&mut self, observer: Arc<dyn ExecutionObserver>) {
        self.observer = Some(observer);
    }
    pub fn enable_trace(&mut self, limit: usize) -> Result<(), RuntimeError> {
        if limit > 1_000_000 {
            return Err(RuntimeError::MemoryLimit);
        }
        self.trace = Some((limit, Vec::new()));
        Ok(())
    }
    pub fn trace(&self) -> &[TraceEvent] {
        self.trace
            .as_ref()
            .map_or(&[], |(_, events)| events.as_slice())
    }
    pub fn steps_used(&self) -> u64 {
        self.steps
    }
    pub fn value_bytes_used(&self) -> u64 {
        self.bytes
    }
    /// Host-supplied exact grants. Document declarations are requirements only.
    pub fn grant(&mut self, capability: Capability) {
        self.capabilities.insert(capability);
    }
    pub fn run_entry(&mut self) -> Result<Vec<Value>, RuntimeError> {
        let entry = self
            .program
            .entry_graph
            .clone()
            .ok_or(RuntimeError::MissingEntry)?;
        self.run_graph(&entry, vec![])
    }
    pub fn run_graph(
        &mut self,
        name: &str,
        inputs: Vec<Value>,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.run_with_host(name, inputs, &mut DenyHost)
    }
    pub fn run_with_host(
        &mut self,
        name: &str,
        inputs: Vec<Value>,
        host: &mut dyn EffectHost,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.steps = 0;
        self.bytes = 0;
        if let Some((_, events)) = &mut self.trace {
            *events = Vec::new();
        }
        for value in &inputs {
            self.charge(value.resident_bytes().ok_or(RuntimeError::MemoryLimit)?)?;
        }
        self.call(name, inputs, 0, host)
    }
    /// Internal bulk execution: one already-validated program and cumulative
    /// budgets across records; no host effects or per-record budget reset.
    pub(crate) fn run_graph_cumulative(
        &mut self,
        name: &str,
        inputs: Vec<Value>,
    ) -> Result<Vec<Value>, RuntimeError> {
        for value in &inputs {
            self.charge(value.resident_bytes().ok_or(RuntimeError::MemoryLimit)?)?;
        }
        self.call(name, inputs, 0, &mut DenyHost)
    }
    fn tick(&mut self) -> Result<(), RuntimeError> {
        if self.cancelled.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        if self.steps >= self.limits.max_steps {
            return Err(RuntimeError::StepLimit);
        }
        self.steps += 1;
        Ok(())
    }
    fn charge(&mut self, bytes: u64) -> Result<(), RuntimeError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .filter(|b| *b <= self.limits.max_value_bytes)
            .ok_or(RuntimeError::MemoryLimit)?;
        Ok(())
    }
    fn call(
        &mut self,
        name: &str,
        inputs: Vec<Value>,
        depth: usize,
        host: &mut dyn EffectHost,
    ) -> Result<Vec<Value>, RuntimeError> {
        self.tick()?;
        if depth >= self.limits.max_call_depth {
            return Err(RuntimeError::CallDepth);
        }
        let graph = self
            .program
            .graphs
            .iter()
            .find(|g| g.name == name)
            .ok_or_else(|| RuntimeError::MissingGraph(name.into()))?;
        let ports = graph
            .nodes
            .iter()
            .try_fold(graph.inputs.len() + graph.outputs.len(), |sum, n| {
                sum.checked_add(n.inputs.len())?
                    .checked_add(n.outputs.len())
            })
            .ok_or(RuntimeError::MemoryLimit)?;
        let metadata = graph
            .nodes
            .len()
            .checked_add(graph.edges.len())
            .and_then(|n| n.checked_add(ports))
            .and_then(|n| n.checked_mul(256))
            .ok_or(RuntimeError::MemoryLimit)?;
        self.charge(metadata as u64)?;
        let mismatch = || RuntimeError::TypeMismatch {
            graph: name.into(),
            node: None,
        };
        let mut ports: Vec<_> = graph.inputs.iter().collect();
        ports.sort_by_key(|p| p.id);
        if inputs.len() != ports.len()
            || !inputs
                .iter()
                .zip(&ports)
                .all(|(v, p)| v.fits(&p.ty, &self.program.schemas))
        {
            return Err(mismatch());
        }
        let mut values: BTreeMap<SourceEndpoint, Value> = ports
            .iter()
            .zip(inputs)
            .map(|(p, v)| (SourceEndpoint::GraphInput(p.id), v))
            .collect();
        let sources: BTreeMap<_, _> = graph.edges.iter().map(|e| (&e.to, &e.from)).collect();
        let mut pending: BTreeMap<_, _> = graph.nodes.iter().map(|n| (n.id, n)).collect();
        let mut dependencies: BTreeMap<_, usize> = graph.nodes.iter().map(|n| (n.id, 0)).collect();
        let mut dependents: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
        for edge in &graph.edges {
            if let (
                SourceEndpoint::NodeOutput { node: source, .. },
                TargetEndpoint::NodeInput { node: target, .. },
            ) = (&edge.from, &edge.to)
            {
                *dependencies
                    .get_mut(target)
                    .ok_or(RuntimeError::InvalidProgram)? += 1;
                dependents.entry(*source).or_default().push(*target);
            }
        }
        let mut ready: BTreeSet<_> = dependencies
            .iter()
            .filter_map(|(id, count)| (*count == 0).then_some(*id))
            .collect();
        while let Some(id) = ready.pop_first() {
            let node = pending.remove(&id).ok_or(RuntimeError::InvalidProgram)?;
            self.tick()?;
            if let Some(observer) = &self.observer {
                observer.before_node(name, node.id, &self.cancelled)?;
            }
            if self.cancelled.is_cancelled() {
                return Err(RuntimeError::Cancelled);
            }
            for required in &node.required_capabilities {
                if !self.capabilities.contains(required) {
                    return Err(RuntimeError::MissingCapability(required.clone()));
                }
            }
            let mut ports: Vec<_> = node.inputs.iter().collect();
            ports.sort_by_key(|p| p.id);
            let args: Vec<_> = ports
                .iter()
                .map(|p| {
                    values[*sources
                        .get(&TargetEndpoint::NodeInput {
                            node: node.id,
                            port: p.id,
                        })
                        .unwrap()]
                    .clone()
                })
                .collect();
            let outputs = self.evaluate(graph, node, args, depth, host)?;
            let mut ports: Vec<_> = node.outputs.iter().collect();
            ports.sort_by_key(|p| p.id);
            if outputs.len() != ports.len()
                || !outputs
                    .iter()
                    .zip(&ports)
                    .all(|(v, p)| v.fits(&p.ty, &self.program.schemas))
            {
                return Err(RuntimeError::TypeMismatch {
                    graph: name.into(),
                    node: Some(node.id),
                });
            }
            if self.trace.is_some() || self.observer.is_some() {
                if self
                    .trace
                    .as_ref()
                    .is_some_and(|(limit, events)| events.len() >= *limit)
                {
                    return Err(RuntimeError::TraceLimit);
                }
                let bytes = outputs
                    .iter()
                    .try_fold(
                        graph.name.len() as u64 + 2 * std::mem::size_of::<TraceEvent>() as u64,
                        |sum, v| sum.checked_add(v.resident_bytes()?),
                    )
                    .ok_or(RuntimeError::MemoryLimit)?;
                self.charge(bytes)?;
                let event = TraceEvent {
                    graph: graph.name.clone(),
                    node: node.id,
                    outputs: outputs.clone(),
                };
                if let Some(observer) = &self.observer {
                    observer.after_node(&event)?;
                }
                if let Some((_, events)) = &mut self.trace {
                    events.push(event);
                }
            }
            for (port, value) in ports.into_iter().zip(outputs) {
                self.charge(value.resident_bytes().ok_or(RuntimeError::MemoryLimit)?)?;
                values.insert(
                    SourceEndpoint::NodeOutput {
                        node: node.id,
                        port: port.id,
                    },
                    value,
                );
            }
            if let Some(children) = dependents.get(&id) {
                for child in children {
                    let count = dependencies.get_mut(child).unwrap();
                    *count -= 1;
                    if *count == 0 {
                        ready.insert(*child);
                    }
                }
            }
        }
        if !pending.is_empty() {
            return Err(RuntimeError::InvalidProgram);
        }
        let mut ports: Vec<_> = graph.outputs.iter().collect();
        ports.sort_by_key(|p| p.id);
        ports
            .into_iter()
            .map(|p| {
                let value = values
                    .get(
                        *sources
                            .get(&TargetEndpoint::GraphOutput(p.id))
                            .ok_or(RuntimeError::InvalidProgram)?,
                    )
                    .ok_or(RuntimeError::InvalidProgram)?
                    .clone();
                if !value.fits(&p.ty, &self.program.schemas) {
                    return Err(mismatch());
                }
                Ok(value)
            })
            .collect()
    }

    fn evaluate(
        &mut self,
        graph: &Graph,
        node: &Node,
        args: Vec<Value>,
        depth: usize,
        host: &mut dyn EffectHost,
    ) -> Result<Vec<Value>, RuntimeError> {
        let bad = || RuntimeError::TypeMismatch {
            graph: graph.name.clone(),
            node: Some(node.id),
        };
        let arithmetic = || RuntimeError::Arithmetic {
            graph: graph.name.clone(),
            node: node.id,
        };
        let integer = |index: usize| match args.get(index) {
            Some(Value::Integer(v)) => Ok(*v),
            _ => Err(bad()),
        };
        let boolean = |index: usize| match args.get(index) {
            Some(Value::Bool(v)) => Ok(*v),
            _ => Err(bad()),
        };
        let value = match &node.operation {
            Operation::Const(literal) => {
                let size = match literal {
                    Literal::Text(v) => v.len(),
                    Literal::Bytes(v) => v.len(),
                    _ => 0,
                };
                self.charge(size as u64)?;
                Value::from(literal)
            }
            Operation::CheckedAdd | Operation::CheckedSub | Operation::CheckedMul => {
                let (a, b) = (integer(0)?, integer(1)?);
                Value::Result(
                    match node.operation {
                        Operation::CheckedAdd => a.checked_add(b),
                        Operation::CheckedSub => a.checked_sub(b),
                        _ => a.checked_mul(b),
                    }
                    .map(|n| Arc::new(Value::Integer(n)))
                    .ok_or_else(|| Arc::new(Value::Bool(true))),
                )
            }
            Operation::ResultIsOk => {
                let Value::Result(result) = &args[0] else {
                    return Err(bad());
                };
                Value::Bool(result.is_ok())
            }
            Operation::Add | Operation::Sub | Operation::Mul | Operation::Div | Operation::Rem => {
                let (a, b) = (integer(0)?, integer(1)?);
                Value::Integer(
                    match node.operation {
                        Operation::Add => a.checked_add(b),
                        Operation::Sub => a.checked_sub(b),
                        Operation::Mul => a.checked_mul(b),
                        Operation::Div => a.checked_div(b),
                        _ => a.checked_rem(b),
                    }
                    .ok_or_else(arithmetic)?,
                )
            }
            Operation::Eq | Operation::Lt | Operation::Le | Operation::Gt | Operation::Ge => {
                let (a, b) = (integer(0)?, integer(1)?);
                Value::Bool(match node.operation {
                    Operation::Eq => a == b,
                    Operation::Lt => a < b,
                    Operation::Le => a <= b,
                    Operation::Gt => a > b,
                    _ => a >= b,
                })
            }
            Operation::And | Operation::Or | Operation::Xor => {
                let (a, b) = (boolean(0)?, boolean(1)?);
                Value::Bool(match node.operation {
                    Operation::And => a & b,
                    Operation::Or => a | b,
                    _ => a ^ b,
                })
            }
            Operation::Not => Value::Bool(!boolean(0)?),
            Operation::ConvertChecked => Value::Integer(integer(0)?),
            Operation::MakeArray => {
                self.charge(
                    (args.len() as u64)
                        .checked_mul(std::mem::size_of::<Value>() as u64)
                        .ok_or(RuntimeError::MemoryLimit)?,
                )?;
                Value::Array(args.into())
            }
            Operation::Index => {
                let index = usize::try_from(integer(1)?).ok();
                let value = match &args[0] {
                    Value::Array(values) => index.and_then(|i| values.get(i)).cloned(),
                    Value::Bytes(bytes) => index
                        .and_then(|i| bytes.get(i))
                        .map(|v| Value::Integer(i128::from(*v))),
                    _ => return Err(bad()),
                };
                Value::Option(value.map(Arc::new))
            }
            Operation::Length => Value::Integer(match &args[0] {
                Value::Array(v) => v.len() as i128,
                Value::Bytes(v) => v.len() as i128,
                _ => return Err(bad()),
            }),
            Operation::TextConcat => {
                let (Value::Text(a), Value::Text(b)) = (&args[0], &args[1]) else {
                    return Err(bad());
                };
                let len = a
                    .len()
                    .checked_add(b.len())
                    .ok_or(RuntimeError::MemoryLimit)?;
                self.charge(len as u64)?;
                let mut text = String::with_capacity(len);
                text.push_str(a);
                text.push_str(b);
                Value::Text(text.into())
            }
            Operation::TextJoin => {
                let (Value::Array(items), Value::Text(separator)) = (&args[0], &args[1]) else {
                    return Err(bad());
                };
                let mut length = separator
                    .len()
                    .checked_mul(items.len().saturating_sub(1))
                    .ok_or(RuntimeError::MemoryLimit)?;
                for item in items.iter() {
                    let Value::Text(text) = item else {
                        return Err(bad());
                    };
                    length = length
                        .checked_add(text.len())
                        .ok_or(RuntimeError::MemoryLimit)?;
                }
                self.charge(length as u64)?;
                let mut joined = String::with_capacity(length);
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        joined.push_str(separator);
                    }
                    let Value::Text(text) = item else {
                        return Err(bad());
                    };
                    joined.push_str(text);
                }
                Value::Text(joined.into())
            }
            Operation::Map { body } => {
                let length = match &args[0] {
                    Value::Bytes(items) => items.len(),
                    Value::Array(items) => items.len(),
                    _ => return Err(bad()),
                };
                if length as u64 > self.limits.max_steps.saturating_sub(self.steps) {
                    return Err(RuntimeError::StepLimit);
                }
                self.charge(
                    (length as u64)
                        .checked_mul(std::mem::size_of::<Value>() as u64)
                        .ok_or(RuntimeError::MemoryLimit)?,
                )?;
                let mut results = Vec::with_capacity(length);
                for index in 0..length {
                    self.tick()?;
                    let value = match &args[0] {
                        Value::Bytes(items) => Value::Integer(items[index] as i128),
                        Value::Array(items) => items[index].clone(),
                        _ => return Err(bad()),
                    };
                    let mut outputs = self.call(body, vec![value], depth + 1, host)?;
                    if outputs.len() != 1 {
                        return Err(bad());
                    }
                    results.push(outputs.remove(0));
                }
                Value::Array(results.into())
            }
            Operation::BytesConcat => {
                let (Value::Bytes(a), Value::Bytes(b)) = (&args[0], &args[1]) else {
                    return Err(bad());
                };
                let len = a
                    .len()
                    .checked_add(b.len())
                    .ok_or(RuntimeError::MemoryLimit)?;
                self.charge(len as u64)?;
                let mut bytes = Vec::with_capacity(len);
                bytes.extend_from_slice(a);
                bytes.extend_from_slice(b);
                Value::Bytes(bytes.into())
            }
            Operation::BytesSlice => {
                let Value::Bytes(bytes) = &args[0] else {
                    return Err(bad());
                };
                let bounds = || RuntimeError::Bounds {
                    graph: graph.name.clone(),
                    node: node.id,
                };
                let start = usize::try_from(integer(1)?).map_err(|_| bounds())?;
                let length = usize::try_from(integer(2)?).map_err(|_| bounds())?;
                let end = start.checked_add(length).ok_or_else(bounds)?;
                let slice = bytes.get(start..end).ok_or_else(bounds)?;
                self.charge(length as u64)?;
                Value::Bytes(slice.into())
            }
            Operation::ArrayConcat => {
                let (Value::Array(a), Value::Array(b)) = (&args[0], &args[1]) else {
                    return Err(bad());
                };
                let length = a
                    .len()
                    .checked_add(b.len())
                    .ok_or(RuntimeError::MemoryLimit)?;
                self.charge(
                    (length as u64)
                        .checked_mul(std::mem::size_of::<Value>() as u64)
                        .ok_or(RuntimeError::MemoryLimit)?,
                )?;
                let mut items = Vec::with_capacity(length);
                items.extend(a.iter().cloned());
                items.extend(b.iter().cloned());
                Value::Array(items.into())
            }
            Operation::Range => {
                let length = usize::try_from(integer(0)?).map_err(|_| RuntimeError::MemoryLimit)?;
                self.charge(
                    (length as u64)
                        .checked_mul(std::mem::size_of::<Value>() as u64)
                        .ok_or(RuntimeError::MemoryLimit)?,
                )?;
                if length as u64 > self.limits.max_steps.saturating_sub(self.steps) {
                    return Err(RuntimeError::StepLimit);
                }
                let mut items = Vec::with_capacity(length);
                for index in 0..length {
                    self.tick()?;
                    items.push(Value::Integer(index as i128));
                }
                Value::Array(items.into())
            }
            Operation::BytesFromArray => {
                let Value::Array(items) = &args[0] else {
                    return Err(bad());
                };
                self.charge(items.len() as u64)?;
                if items.len() as u64 > self.limits.max_steps.saturating_sub(self.steps) {
                    return Err(RuntimeError::StepLimit);
                }
                let mut bytes = Vec::with_capacity(items.len());
                for value in items.iter() {
                    self.tick()?;
                    let Value::Integer(value) = value else {
                        return Err(bad());
                    };
                    bytes.push(u8::try_from(*value).map_err(|_| bad())?);
                }
                Value::Bytes(bytes.into())
            }
            Operation::EncodeUtf8 => {
                let Value::Text(text) = &args[0] else {
                    return Err(bad());
                };
                self.charge(text.len() as u64)?;
                Value::Bytes(text.as_bytes().into())
            }
            Operation::DecodeUtf8 => {
                let Value::Bytes(bytes) = &args[0] else {
                    return Err(bad());
                };
                Value::Result(match std::str::from_utf8(bytes) {
                    Ok(text) => {
                        self.charge(text.len() as u64)?;
                        Ok(Arc::new(Value::Text(text.into())))
                    }
                    Err(_) => Err(Arc::new(args[0].clone())),
                })
            }
            Operation::FormatInteger => {
                self.charge(40)?;
                Value::Text(integer(0)?.to_string().into())
            }
            Operation::MakeRecord { schema, fields } => {
                self.charge(
                    fields
                        .iter()
                        .try_fold(schema.len() as u64, |sum, f| {
                            sum.checked_add(f.len() as u64 + std::mem::size_of::<Value>() as u64)
                        })
                        .ok_or(RuntimeError::MemoryLimit)?,
                )?;
                Value::Record {
                    schema: schema.clone(),
                    fields: Arc::new(fields.iter().cloned().zip(args).collect()),
                }
            }
            Operation::Field { name } => {
                let Value::Record { schema, fields } = &args[0] else {
                    return Err(bad());
                };
                let field = self
                    .program
                    .schemas
                    .iter()
                    .find(|s| &s.name == schema)
                    .and_then(|s| s.fields.iter().find(|f| &f.name == name))
                    .ok_or_else(bad)?;
                match field.requirement {
                    crate::data_format::FieldRequirement::Optional => {
                        Value::Option(fields.get(name).cloned().map(Arc::new))
                    }
                    crate::data_format::FieldRequirement::Required => {
                        fields.get(name).ok_or_else(bad)?.clone()
                    }
                }
            }
            Operation::MakeVariant { schema, tag } => {
                self.charge((schema.len() + tag.len()) as u64)?;
                Value::Variant {
                    schema: schema.clone(),
                    tag: tag.clone(),
                    payload: Arc::new(args[0].clone()),
                }
            }
            Operation::VariantPayload { tag } => {
                let Value::Variant {
                    tag: actual,
                    payload,
                    ..
                } = &args[0]
                else {
                    return Err(bad());
                };
                Value::Option((tag == actual).then(|| payload.clone()))
            }
            Operation::Some => Value::Option(Some(Arc::new(args[0].clone()))),
            Operation::None => Value::Option(None),
            Operation::Ok => Value::Result(Ok(Arc::new(args[0].clone()))),
            Operation::Err => Value::Result(Err(Arc::new(args[0].clone()))),
            Operation::UnwrapOr => match &args[0] {
                Value::Option(value) => value.as_deref().unwrap_or(&args[1]).clone(),
                Value::Result(Ok(value)) => value.as_ref().clone(),
                Value::Result(Err(_)) => args[1].clone(),
                _ => return Err(bad()),
            },
            Operation::Truncate { bits, signed } => {
                let shift = 128 - u32::from(*bits);
                let value = integer(0)?;
                Value::Integer(if *signed {
                    (value << shift) >> shift
                } else {
                    value & ((1i128 << bits) - 1)
                })
            }
            Operation::Subgraph(target) => return self.call(target, args, depth + 1, host),
            Operation::Select {
                when_true,
                when_false,
            } => {
                let target = if boolean(0)? { when_true } else { when_false };
                return self.call(target, args[1..].to_vec(), depth + 1, host);
            }
            Operation::Match { arms, default } => {
                let Some(Value::Variant { tag, .. }) = args.first() else {
                    return Err(bad());
                };
                let target = arms
                    .iter()
                    .find(|a| &a.tag == tag)
                    .map_or(default, |a| &a.graph);
                return self.call(target, args[1..].to_vec(), depth + 1, host);
            }
            Operation::Loop {
                condition,
                body,
                max_iterations,
            } => {
                let mut state = args;
                for iteration in 0..=*max_iterations {
                    self.tick()?;
                    let test = self.call(condition, state.clone(), depth + 1, host)?;
                    if test == [Value::Bool(false)] {
                        return Ok(state);
                    }
                    if test != [Value::Bool(true)] {
                        return Err(bad());
                    }
                    if iteration == *max_iterations {
                        return Err(RuntimeError::LoopLimit {
                            graph: graph.name.clone(),
                            node: node.id,
                        });
                    }
                    state = self.call(body, state, depth + 1, host)?;
                }
                unreachable!()
            }
            Operation::Import(_) => return Ok(vec![]),
            _ if !node.effects.is_empty() => return host.execute(node, &args),
            _ => {
                return Err(RuntimeError::Unsupported {
                    graph: graph.name.clone(),
                    node: node.id,
                });
            }
        };
        Ok(vec![value])
    }
}
