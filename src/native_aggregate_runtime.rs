//! Bounded immutable values for generated native code. This module implements
//! individual primitives; it never schedules or interprets a graph.
use crate::{
    execution::{Cancellation, ExecutionLimits, RuntimeError},
    gir::{Literal, Operation},
    program::{PlatformContract, ProgramContract, validate_program},
    value::Value,
};
use std::sync::Arc;
const PACK_TAG: u64 = 1 << 63;
const SCALAR_MIN: i128 = -32;
const SCALAR_MAX: i128 = 1023;
const INTEGER_SCALAR_SLOTS: usize = (SCALAR_MAX - SCALAR_MIN + 1) as usize;
const SCALAR_SLOTS: usize = INTEGER_SCALAR_SLOTS + 2;
fn pack_index(pack: u64) -> Option<usize> {
    if pack & PACK_TAG == 0 {
        return None;
    }
    usize::try_from((pack & !PACK_TAG).checked_sub(1)?).ok()
}

pub struct NativeContext {
    program: ProgramContract,
    values: Vec<Value>,
    // Fixed storage avoids retaining a fresh Value for every repeated byte,
    // small arithmetic constant or Bool in a long-running native compiler.
    // Handles denote immutable values, so equal scalars can share a slot.
    scalar_handles: [u64; SCALAR_SLOTS],
    limits: ExecutionLimits,
    steps: u64,
    bytes: u64,
    depth: usize,
    cancelled: Cancellation,
    stack_frames: Vec<u64>,
    stack_bytes: u64,
    builders: Vec<Option<(usize, Vec<Value>)>>,
    packs: Vec<Vec<u64>>,
    pub error: Option<RuntimeError>,
}

impl NativeContext {
    pub fn new(program: ProgramContract, limits: ExecutionLimits) -> Result<Self, RuntimeError> {
        if limits.max_call_depth > 128 {
            return Err(RuntimeError::CallDepth);
        }
        validate_program(&program, &PlatformContract::bootstrap_x86_64_v3())
            .map_err(|_| RuntimeError::InvalidProgram)?;
        Ok(Self {
            program,
            values: Vec::new(),
            scalar_handles: [0; SCALAR_SLOTS],
            limits,
            steps: 0,
            bytes: 0,
            depth: 0,
            cancelled: Cancellation::default(),
            stack_frames: Vec::new(),
            stack_bytes: 0,
            builders: Vec::new(),
            packs: Vec::new(),
            error: None,
        })
    }
    pub fn value(&self, handle: u64) -> Option<&Value> {
        self.values
            .get(usize::try_from(handle.checked_sub(1)?).ok()?)
    }
    pub fn cancellation(&self) -> Cancellation {
        self.cancelled.clone()
    }
    /// Add a host-supplied immutable input under the same resident-value budget.
    pub fn insert_value(&mut self, value: Value) -> Result<u64, RuntimeError> {
        // This arena owns immutable values, not live resources or linear handles.
        if value.contains_native_handles() {
            self.fail(RuntimeError::InvalidHostResult);
            return Err(RuntimeError::InvalidHostResult);
        }
        match self.put(value) {
            Ok(h) => Ok(h),
            Err(e) => {
                self.fail(e.clone());
                Err(e)
            }
        }
    }
    fn fail(&mut self, issue: RuntimeError) -> u64 {
        if self.error.is_none() {
            self.error = Some(issue);
        }
        0
    }
    fn reserve(&self, bytes: u64) -> Result<(), RuntimeError> {
        if bytes
            > self
                .limits
                .max_value_bytes
                .saturating_sub(self.bytes)
                .saturating_sub(self.stack_bytes)
        {
            Err(RuntimeError::MemoryLimit)
        } else {
            Ok(())
        }
    }
    fn tick(&mut self) -> Result<(), RuntimeError> {
        if self.error.is_some() {
            return Err(self.error.clone().unwrap());
        }
        if self.cancelled.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        if self.steps >= self.limits.max_steps {
            return Err(RuntimeError::StepLimit);
        }
        self.steps += 1;
        Ok(())
    }
    fn put(&mut self, value: Value) -> Result<u64, RuntimeError> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        let bytes = value.resident_bytes().ok_or(RuntimeError::MemoryLimit)?;
        self.reserve(bytes)?;
        self.bytes += bytes;
        // Charge each production exactly as before, even if physical storage
        // can be reused. Interning must not weaken cumulative memory limits.
        let scalar_slot = match &value {
            Value::Integer(n) if (SCALAR_MIN..=SCALAR_MAX).contains(n) => {
                Some((*n - SCALAR_MIN) as usize)
            }
            Value::Bool(v) => Some(INTEGER_SCALAR_SLOTS + usize::from(*v)),
            _ => None,
        };
        if let Some(slot) = scalar_slot {
            let handle = self.scalar_handles[slot];
            if handle != 0 {
                return Ok(handle);
            }
        }
        self.values.push(value);
        let handle = self.values.len() as u64;
        if let Some(slot) = scalar_slot {
            self.scalar_handles[slot] = handle;
        }
        Ok(handle)
    }
    pub fn primitive(&mut self, graph: usize, node: usize, handles: &[u64]) -> u64 {
        let result = self.primitive_inner(graph, node, handles);
        match result {
            Ok(value) => value,
            Err(issue) => self.fail(issue),
        }
    }
    fn primitive_inner(
        &mut self,
        graph: usize,
        node: usize,
        handles: &[u64],
    ) -> Result<u64, RuntimeError> {
        self.tick()?;
        let graph = self
            .program
            .graphs
            .get(graph)
            .ok_or(RuntimeError::InvalidProgram)?;
        let node = graph
            .nodes
            .get(node)
            .ok_or(RuntimeError::InvalidProgram)?
            .clone();
        let graph_name = graph.name.clone();
        let bad = || RuntimeError::TypeMismatch {
            graph: graph_name.clone(),
            node: Some(node.id),
        };
        let arithmetic = || RuntimeError::Arithmetic {
            graph: graph_name.clone(),
            node: node.id,
        };
        let bounds = || RuntimeError::Bounds {
            graph: graph_name.clone(),
            node: node.id,
        };
        if handles.len() != node.inputs.len() || node.outputs.len() != 1 {
            return Err(bad());
        }
        if let Some(capability) = node.required_capabilities.iter().next() {
            return Err(RuntimeError::MissingCapability(capability.clone()));
        }
        if !node.effects.is_empty() {
            return Err(RuntimeError::Unsupported {
                graph: graph_name,
                node: node.id,
            });
        }
        let args: Vec<Value> = handles
            .iter()
            .map(|handle| self.value(*handle).cloned().ok_or_else(bad))
            .collect::<Result<_, _>>()?;
        if !args
            .iter()
            .zip(&node.inputs)
            .all(|(value, port)| value.fits(&port.ty, &self.program.schemas))
        {
            return Err(bad());
        }
        let integer = |i: usize| match args.get(i) {
            Some(Value::Integer(v)) => Ok(*v),
            _ => Err(bad()),
        };
        let boolean = |i: usize| match args.get(i) {
            Some(Value::Bool(v)) => Ok(*v),
            _ => Err(bad()),
        };
        let array_bytes = |n: usize| {
            (n as u64)
                .checked_mul(std::mem::size_of::<Value>() as u64)
                .ok_or(RuntimeError::MemoryLimit)
        };
        let value = match &node.operation {
            Operation::Const(literal) => {
                match literal {
                    Literal::Text(v) => self.reserve(v.len() as u64)?,
                    Literal::Bytes(v) => self.reserve(v.len() as u64)?,
                    _ => {}
                }
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
            Operation::Add => Value::Integer(
                integer(0)?
                    .checked_add(integer(1)?)
                    .ok_or_else(arithmetic)?,
            ),
            Operation::Sub => Value::Integer(
                integer(0)?
                    .checked_sub(integer(1)?)
                    .ok_or_else(arithmetic)?,
            ),
            Operation::Mul => Value::Integer(
                integer(0)?
                    .checked_mul(integer(1)?)
                    .ok_or_else(arithmetic)?,
            ),
            Operation::Div => Value::Integer(
                integer(0)?
                    .checked_div(integer(1)?)
                    .ok_or_else(arithmetic)?,
            ),
            Operation::Rem => Value::Integer(
                integer(0)?
                    .checked_rem(integer(1)?)
                    .ok_or_else(arithmetic)?,
            ),
            Operation::Eq => Value::Bool(args[0] == args[1]),
            Operation::Lt => Value::Bool(integer(0)? < integer(1)?),
            Operation::Le => Value::Bool(integer(0)? <= integer(1)?),
            Operation::Gt => Value::Bool(integer(0)? > integer(1)?),
            Operation::Ge => Value::Bool(integer(0)? >= integer(1)?),
            Operation::And => Value::Bool(boolean(0)? & boolean(1)?),
            Operation::Or => Value::Bool(boolean(0)? | boolean(1)?),
            Operation::Xor => Value::Bool(boolean(0)? ^ boolean(1)?),
            Operation::Not => Value::Bool(!boolean(0)?),
            Operation::ConvertChecked => Value::Integer(integer(0)?),
            Operation::Truncate { bits, signed } => {
                let shift = 128 - u32::from(*bits);
                let v = integer(0)?;
                Value::Integer(if *signed {
                    (v << shift) >> shift
                } else if *bits == 128 {
                    v
                } else {
                    v & ((1i128 << bits) - 1)
                })
            }
            Operation::MakeArray => {
                self.reserve(array_bytes(args.len())?)?;
                Value::Array(args.clone().into())
            }
            Operation::Index => {
                let index = usize::try_from(integer(1)?).ok();
                let value = match &args[0] {
                    Value::Array(v) => index.and_then(|i| v.get(i)).cloned(),
                    Value::Bytes(v) => index
                        .and_then(|i| v.get(i))
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
                self.reserve(
                    (a.len() as u64)
                        .checked_add(b.len() as u64)
                        .ok_or(RuntimeError::MemoryLimit)?,
                )?;
                Value::Text(format!("{a}{b}").into())
            }
            Operation::BytesConcat => {
                let (Value::Bytes(a), Value::Bytes(b)) = (&args[0], &args[1]) else {
                    return Err(bad());
                };
                self.reserve(
                    (a.len() as u64)
                        .checked_add(b.len() as u64)
                        .ok_or(RuntimeError::MemoryLimit)?,
                )?;
                let mut v = a.to_vec();
                v.extend_from_slice(b);
                Value::Bytes(v.into())
            }
            Operation::ArrayConcat => {
                let (Value::Array(a), Value::Array(b)) = (&args[0], &args[1]) else {
                    return Err(bad());
                };
                let n = a
                    .len()
                    .checked_add(b.len())
                    .ok_or(RuntimeError::MemoryLimit)?;
                self.reserve(array_bytes(n)?)?;
                let mut v = a.to_vec();
                v.extend(b.iter().cloned());
                Value::Array(v.into())
            }
            Operation::BytesSlice => {
                let Value::Bytes(v) = &args[0] else {
                    return Err(bad());
                };
                let start = usize::try_from(integer(1)?).map_err(|_| bounds())?;
                let len = usize::try_from(integer(2)?).map_err(|_| bounds())?;
                let end = start.checked_add(len).ok_or_else(bounds)?;
                let slice = v.get(start..end).ok_or_else(bounds)?;
                self.reserve(len as u64)?;
                Value::Bytes(slice.into())
            }
            Operation::Range => {
                let len = usize::try_from(integer(0)?).map_err(|_| RuntimeError::MemoryLimit)?;
                self.reserve(array_bytes(len)?)?;
                if len as u64 > self.limits.max_steps.saturating_sub(self.steps) {
                    return Err(RuntimeError::StepLimit);
                };
                let mut items = Vec::with_capacity(len);
                for i in 0..len {
                    self.tick()?;
                    items.push(Value::Integer(i as i128))
                }
                Value::Array(items.into())
            }
            Operation::BytesFromArray => {
                let Value::Array(v) = &args[0] else {
                    return Err(bad());
                };
                self.reserve(v.len() as u64)?;
                if v.len() as u64 > self.limits.max_steps.saturating_sub(self.steps) {
                    return Err(RuntimeError::StepLimit);
                };
                let mut bytes = Vec::with_capacity(v.len());
                for value in v.iter() {
                    self.tick()?;
                    let Value::Integer(value) = value else {
                        return Err(bad());
                    };
                    bytes.push(u8::try_from(*value).map_err(|_| bad())?)
                }
                Value::Bytes(bytes.into())
            }
            Operation::EncodeUtf8 => {
                let Value::Text(v) = &args[0] else {
                    return Err(bad());
                };
                self.reserve(v.len() as u64)?;
                Value::Bytes(v.as_bytes().into())
            }
            Operation::DecodeUtf8 => {
                let Value::Bytes(v) = &args[0] else {
                    return Err(bad());
                };
                self.reserve(v.len() as u64)?;
                Value::Result(match std::str::from_utf8(v) {
                    Ok(v) => Ok(Arc::new(Value::Text(v.into()))),
                    Err(_) => Err(Arc::new(args[0].clone())),
                })
            }
            Operation::DecodeInteger128Le => {
                let Value::Bytes(bytes) = &args[0] else {
                    return Err(bad());
                };
                let bytes: [u8; 16] = bytes.as_ref().try_into().map_err(|_| bounds())?;
                Value::Integer(i128::from_le_bytes(bytes))
            }
            Operation::FormatInteger => {
                self.reserve(40)?;
                Value::Text(integer(0)?.to_string().into())
            }
            Operation::TextJoin => {
                let (Value::Array(v), Value::Text(separator)) = (&args[0], &args[1]) else {
                    return Err(bad());
                };
                self.reserve(
                    (v.len() as u64)
                        .checked_mul(16)
                        .ok_or(RuntimeError::MemoryLimit)?,
                )?;
                let texts = v
                    .iter()
                    .map(|v| match v {
                        Value::Text(v) => Ok(v.as_ref()),
                        _ => Err(bad()),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let len = texts.iter().try_fold(
                    separator
                        .len()
                        .checked_mul(texts.len().saturating_sub(1))
                        .ok_or(RuntimeError::MemoryLimit)?,
                    |n, s| n.checked_add(s.len()).ok_or(RuntimeError::MemoryLimit),
                )?;
                self.reserve(len as u64)?;
                Value::Text(texts.join(separator.as_ref()).into())
            }
            Operation::MakeRecord { schema, fields } => {
                self.reserve(
                    fields
                        .iter()
                        .try_fold(schema.len() as u64, |sum, field| {
                            sum.checked_add(field.len() as u64).and_then(|n| {
                                n.checked_add(128 + std::mem::size_of::<Value>() as u64)
                            })
                        })
                        .ok_or(RuntimeError::MemoryLimit)?,
                )?;
                Value::Record {
                    schema: schema.clone(),
                    fields: Arc::new(fields.iter().cloned().zip(args.iter().cloned()).collect()),
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
                    crate::data_format::FieldRequirement::Required => {
                        fields.get(name).cloned().ok_or_else(bad)?
                    }
                    crate::data_format::FieldRequirement::Optional => {
                        Value::Option(fields.get(name).cloned().map(Arc::new))
                    }
                }
            }
            Operation::MakeVariant { schema, tag } => {
                self.reserve(
                    (schema.len() as u64)
                        .checked_add(tag.len() as u64)
                        .and_then(|n| n.checked_add(std::mem::size_of::<Value>() as u64))
                        .ok_or(RuntimeError::MemoryLimit)?,
                )?;
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
                Value::Option(v) => v.as_deref().unwrap_or(&args[1]).clone(),
                Value::Result(Ok(v)) => v.as_ref().clone(),
                Value::Result(Err(_)) => args[1].clone(),
                _ => return Err(bad()),
            },
            _ => {
                return Err(RuntimeError::Unsupported {
                    graph: graph_name,
                    node: node.id,
                });
            }
        };
        if !value.fits(&node.outputs[0].ty, &self.program.schemas) {
            return Err(bad());
        }
        self.put(value)
    }
}

/// # Safety
/// `context` must be an exclusive live context pointer, and `args` must point
/// to `count` initialized handles (or be null for a zero-length argument list).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_primitive(
    context: *mut NativeContext,
    graph: u64,
    node: u64,
    args: *const u64,
    count: u64,
) -> u64 {
    let Some(context) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    let Ok(count) = usize::try_from(count) else {
        return context.fail(RuntimeError::InvalidProgram);
    };
    if count > context.limits.max_value_bytes as usize / 8 || (count > 0 && args.is_null()) {
        return context.fail(RuntimeError::InvalidProgram);
    }
    let args = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(args, count) }
    };
    context.primitive(graph as usize, node as usize, args)
}
/// # Safety
/// `context` must be an exclusive live context pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_enter(context: *mut NativeContext, graph: u64) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    if c.error.is_some() {
        return 0;
    }
    if graph as usize >= c.program.graphs.len() {
        return c.fail(RuntimeError::InvalidProgram);
    }
    if let Err(e) = c.tick() {
        return c.fail(e);
    }
    if c.depth >= c.limits.max_call_depth {
        return c.fail(RuntimeError::CallDepth);
    }
    let g = &c.program.graphs[graph as usize];
    let Some(slots) = g
        .nodes
        .iter()
        .try_fold(g.inputs.len(), |sum, n| sum.checked_add(n.outputs.len()))
    else {
        return c.fail(RuntimeError::MemoryLimit);
    };
    let args = g
        .nodes
        .iter()
        .map(|n| n.inputs.len().max(n.outputs.len()))
        .max()
        .unwrap_or(0)
        .max(g.outputs.len())
        .max(1);
    let Some(frame) = slots
        .checked_add(args)
        // Reserve saved registers and control locals for both native emitters.
        // The graph-compiler ABI uses four callee-saved registers and six
        // control slots, in addition to alignment and fixed frame metadata.
        .and_then(|n| n.checked_add(16))
        .and_then(|n| n.checked_mul(8))
        .map(|n| (n as u64).div_ceil(16) * 16)
    else {
        return c.fail(RuntimeError::MemoryLimit);
    };
    if c.stack_bytes.saturating_add(frame) > 4 * 1024 * 1024 {
        return c.fail(RuntimeError::CallDepth);
    }
    if let Err(e) = c.reserve(frame) {
        return c.fail(e);
    }
    c.stack_bytes += frame;
    c.stack_frames.push(frame);
    c.depth += 1;
    1
}
/// # Safety
/// `context` must be an exclusive live context pointer after a successful enter.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_leave(context: *mut NativeContext) {
    if let Some(c) = unsafe { context.as_mut() } {
        c.depth = c.depth.saturating_sub(1);
        if let Some(frame) = c.stack_frames.pop() {
            c.stack_bytes = c.stack_bytes.saturating_sub(frame)
        }
    }
}
/// # Safety
/// `context` must be an exclusive live context pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_truth(context: *mut NativeContext, handle: u64) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    match c.value(handle) {
        Some(Value::Bool(v)) => u64::from(*v),
        _ => c.fail(RuntimeError::InvalidProgram),
    }
}
/// # Safety
/// `context` must be an exclusive live context pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_failed(context: *mut NativeContext) -> u64 {
    unsafe { context.as_ref() }.map_or(1, |c| u64::from(c.error.is_some()))
}
/// # Safety
/// `context` must be an exclusive live context pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_integer(
    context: *mut NativeContext,
    low: u64,
    high: u64,
) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    let value = Value::Integer(((high as i128) << 64) | (low as i128));
    match c.put(value) {
        Ok(h) => h,
        Err(e) => c.fail(e),
    }
}
/// # Safety
/// `context` must be an exclusive live context pointer; `value` is zero or one.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_bool(context: *mut NativeContext, value: u64) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    if value > 1 {
        return c.fail(RuntimeError::InvalidProgram);
    }
    match c.put(Value::Bool(value != 0)) {
        Ok(h) => h,
        Err(e) => c.fail(e),
    }
}
/// # Safety
/// `context` must be an exclusive live context pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_result_integer(context: *mut NativeContext, handle: u64) -> i64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    let result = match c.value(handle) {
        Some(Value::Integer(v)) => i64::try_from(*v).ok(),
        Some(Value::Bool(v)) => Some(i64::from(*v)),
        _ => None,
    };
    result.unwrap_or_else(|| {
        c.fail(RuntimeError::InvalidProgram);
        0
    })
}

/// # Safety
/// `bytes` must point to `length` initialized bytes of a native program document.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_context_new(
    bytes: *const u8,
    length: u64,
    steps: u64,
    memory: u64,
    depth: u64,
) -> *mut NativeContext {
    if bytes.is_null() || length > isize::MAX as u64 {
        return std::ptr::null_mut();
    }
    let data = unsafe { std::slice::from_raw_parts(bytes, length as usize) };
    let Ok(document) = crate::program_binary::decode_program(data) else {
        return std::ptr::null_mut();
    };
    let Ok(program) = document.validated_contract() else {
        return std::ptr::null_mut();
    };
    match NativeContext::new(
        program,
        ExecutionLimits {
            max_steps: steps,
            max_value_bytes: memory,
            max_call_depth: usize::try_from(depth).unwrap_or(usize::MAX),
        },
    ) {
        Ok(c) => Box::into_raw(Box::new(c)),
        Err(_) => std::ptr::null_mut(),
    }
}
/// # Safety
/// Pointer must come from `g0_native_context_new` and must be freed exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_context_free(context: *mut NativeContext) {
    if !context.is_null() {
        drop(unsafe { Box::from_raw(context) })
    }
}
/// # Safety
/// Context must be live and exclusive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_sequence_len(context: *mut NativeContext, handle: u64) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    match c.value(handle) {
        Some(Value::Array(v)) => v.len() as u64,
        Some(Value::Bytes(v)) => v.len() as u64,
        _ => c.fail(RuntimeError::InvalidProgram),
    }
}
/// # Safety
/// Context must be live and exclusive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_sequence_item(
    context: *mut NativeContext,
    handle: u64,
    index: u64,
) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    if let Err(e) = c.tick() {
        return c.fail(e);
    }
    let value = match c.value(handle) {
        Some(Value::Array(v)) => v.get(index as usize).cloned(),
        Some(Value::Bytes(v)) => v.get(index as usize).map(|v| Value::Integer(*v as i128)),
        _ => None,
    };
    match value {
        Some(v) => match c.put(v) {
            Ok(h) => h,
            Err(e) => c.fail(e),
        },
        None => c.fail(RuntimeError::InvalidProgram),
    }
}
/// # Safety
/// Context must be live and exclusive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_map_begin(context: *mut NativeContext, length: u64) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    if c.error.is_some() {
        return 0;
    }
    // The builder table retains its slot after finish, including for an empty
    // map. Charge metadata and spare table capacity before either allocation.
    let Some(bytes) = length
        .checked_mul(std::mem::size_of::<Value>() as u64)
        .and_then(|n| n.checked_add(2 * std::mem::size_of::<Option<(usize, Vec<Value>)>>() as u64))
    else {
        return c.fail(RuntimeError::MemoryLimit);
    };
    if let Err(e) = c.reserve(bytes) {
        return c.fail(e);
    }
    if length > c.limits.max_steps.saturating_sub(c.steps) {
        return c.fail(RuntimeError::StepLimit);
    }
    let Ok(length) = usize::try_from(length) else {
        return c.fail(RuntimeError::MemoryLimit);
    };
    c.bytes += bytes;
    c.builders.push(Some((length, Vec::with_capacity(length))));
    c.builders.len() as u64
}
/// # Safety
/// Context must be live and exclusive; builder comes from map_begin.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_map_push(
    context: *mut NativeContext,
    builder: u64,
    handle: u64,
) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    let Some(value) = c.value(handle).cloned() else {
        return c.fail(RuntimeError::InvalidProgram);
    };
    let Some(Some((length, values))) = builder
        .checked_sub(1)
        .and_then(|b| c.builders.get_mut(b as usize))
    else {
        return c.fail(RuntimeError::InvalidProgram);
    };
    if values.len() >= *length {
        return c.fail(RuntimeError::InvalidProgram);
    }
    values.push(value);
    1
}
/// # Safety
/// Context must be live and exclusive; indices refer to the compiled metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_map_finish(
    context: *mut NativeContext,
    graph: u64,
    node: u64,
    builder: u64,
) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    let Some(slot) = builder
        .checked_sub(1)
        .and_then(|b| c.builders.get_mut(b as usize))
    else {
        return c.fail(RuntimeError::InvalidProgram);
    };
    let Some((length, values)) = slot.take() else {
        return c.fail(RuntimeError::InvalidProgram);
    };
    if length != values.len() {
        return c.fail(RuntimeError::InvalidProgram);
    }
    let value = Value::Array(values.into());
    let Some(node) = c
        .program
        .graphs
        .get(graph as usize)
        .and_then(|g| g.nodes.get(node as usize))
    else {
        return c.fail(RuntimeError::InvalidProgram);
    };
    if node.outputs.len() != 1 || !value.fits(&node.outputs[0].ty, &c.program.schemas) {
        return c.fail(RuntimeError::InvalidProgram);
    }
    match c.put(value) {
        Ok(h) => h,
        Err(e) => c.fail(e),
    }
}
/// # Safety
/// Context must be live and exclusive; indices refer to the compiled metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_loop_fail(
    context: *mut NativeContext,
    graph: u64,
    node: u64,
) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    let issue = match c.program.graphs.get(graph as usize).and_then(|g| {
        g.nodes.get(node as usize).map(|n| RuntimeError::LoopLimit {
            graph: g.name.clone(),
            node: n.id,
        })
    }) {
        Some(e) => e,
        None => RuntimeError::InvalidProgram,
    };
    c.fail(issue)
}

/// # Safety
/// Context is live and exclusive; args points to count initialized handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_graph_enter(
    context: *mut NativeContext,
    graph: u64,
    args: *const u64,
    count: u64,
) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    let Some(g) = c.program.graphs.get(graph as usize) else {
        return c.fail(RuntimeError::InvalidProgram);
    };
    if count as usize != g.inputs.len() || (count > 0 && args.is_null()) {
        return c.fail(RuntimeError::InvalidProgram);
    }
    let handles = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(args, count as usize) }
    };
    let mut ports: Vec<_> = g.inputs.iter().collect();
    ports.sort_by_key(|p| p.id);
    if !handles.iter().zip(ports).all(|(h, p)| {
        c.value(*h)
            .is_some_and(|v| v.fits(&p.ty, &c.program.schemas))
    }) {
        return c.fail(RuntimeError::TypeMismatch {
            graph: g.name.clone(),
            node: None,
        });
    }
    unsafe { g0_native_enter(context, graph) }
}
/// # Safety
/// Context is live and exclusive; indices refer to the compiled metadata.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_graph_result(
    context: *mut NativeContext,
    graph: u64,
    handle: u64,
) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    if c.error.is_some() {
        return 0;
    }
    let Some(g) = c.program.graphs.get(graph as usize) else {
        return c.fail(RuntimeError::InvalidProgram);
    };
    if g.outputs.len() != 1
        || !c
            .value(handle)
            .is_some_and(|v| v.fits(&g.outputs[0].ty, &c.program.schemas))
    {
        return c.fail(RuntimeError::TypeMismatch {
            graph: g.name.clone(),
            node: None,
        });
    }
    handle
}
/// # Safety
/// Context is live and exclusive; args points to count initialized handles.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_control_begin(
    context: *mut NativeContext,
    graph: u64,
    node: u64,
    args: *const u64,
    count: u64,
) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    if let Err(e) = c.tick() {
        return c.fail(e);
    }
    let Some((g, n)) = c
        .program
        .graphs
        .get(graph as usize)
        .and_then(|g| g.nodes.get(node as usize).map(|n| (g, n)))
    else {
        return c.fail(RuntimeError::InvalidProgram);
    };
    if count as usize != n.inputs.len() || (count > 0 && args.is_null()) {
        return c.fail(RuntimeError::InvalidProgram);
    }
    let handles = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(args, count as usize) }
    };
    let mut ports: Vec<_> = n.inputs.iter().collect();
    ports.sort_by_key(|p| p.id);
    if !handles.iter().zip(ports).all(|(h, p)| {
        c.value(*h)
            .is_some_and(|v| v.fits(&p.ty, &c.program.schemas))
    }) {
        return c.fail(RuntimeError::TypeMismatch {
            graph: g.name.clone(),
            node: Some(n.id),
        });
    }
    1
}

/// # Safety
/// Context is live and exclusive; handles points to count initialized handles.
/// Packs are private call-result tuples, never language Array values.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_pack(
    context: *mut NativeContext,
    handles: *const u64,
    count: u64,
) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    if c.error.is_some() {
        return 0;
    }
    // Include the pack-table metadata even for a zero-output call. The factor
    // two also bounds spare capacity in the growing pack table.
    let Some(bytes) = count
        .checked_mul(8)
        .and_then(|n| n.checked_add(2 * std::mem::size_of::<Vec<u64>>() as u64))
    else {
        return c.fail(RuntimeError::MemoryLimit);
    };
    if let Err(e) = c.reserve(bytes) {
        return c.fail(e);
    }
    if count > usize::MAX as u64 || (count > 0 && handles.is_null()) {
        return c.fail(RuntimeError::InvalidProgram);
    }
    let handles = if count == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(handles, count as usize) }
    };
    if !handles.iter().all(|h| c.value(*h).is_some()) {
        return c.fail(RuntimeError::InvalidProgram);
    }
    c.bytes += bytes;
    c.packs.push(handles.to_vec());
    PACK_TAG | c.packs.len() as u64
}
/// # Safety
/// Context is live and exclusive; pack is an internal result tuple.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_pack_item(
    context: *mut NativeContext,
    pack: u64,
    index: u64,
) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    if c.error.is_some() {
        return 0;
    }
    match pack_index(pack)
        .and_then(|p| c.packs.get(p))
        .and_then(|p| p.get(index as usize))
        .copied()
    {
        Some(h) => h,
        None => c.fail(RuntimeError::InvalidProgram),
    }
}
/// # Safety
/// Context is live and exclusive. node=u64::MAX checks the graph outputs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_pack_check(
    context: *mut NativeContext,
    graph: u64,
    node: u64,
    pack: u64,
) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    if c.error.is_some() {
        return 0;
    }
    let Some(g) = c.program.graphs.get(graph as usize) else {
        return c.fail(RuntimeError::InvalidProgram);
    };
    let ports = if node == u64::MAX {
        &g.outputs
    } else {
        let Some(n) = g.nodes.get(node as usize) else {
            return c.fail(RuntimeError::InvalidProgram);
        };
        &n.outputs
    };
    let mut ports: Vec<_> = ports.iter().collect();
    ports.sort_by_key(|p| p.id);
    let Some(handles) = pack_index(pack).and_then(|p| c.packs.get(p)) else {
        return c.fail(RuntimeError::InvalidProgram);
    };
    if handles.len() != ports.len()
        || !handles.iter().zip(ports).all(|(h, p)| {
            c.value(*h)
                .is_some_and(|v| v.fits(&p.ty, &c.program.schemas))
        })
    {
        return c.fail(RuntimeError::TypeMismatch {
            graph: g.name.clone(),
            node: if node == u64::MAX {
                None
            } else {
                g.nodes.get(node as usize).map(|n| n.id)
            },
        });
    }
    pack
}
/// # Safety
/// Context is live and exclusive; graph and node are canonical metadata indices.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_match_arm(
    context: *mut NativeContext,
    graph: u64,
    node: u64,
    selector: u64,
) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return u64::MAX;
    };
    let Some(g) = c.program.graphs.get(graph as usize) else {
        c.fail(RuntimeError::InvalidProgram);
        return u64::MAX;
    };
    let Some(n) = g.nodes.get(node as usize) else {
        c.fail(RuntimeError::InvalidProgram);
        return u64::MAX;
    };
    let (Operation::Match { arms, .. }, Some(Value::Variant { tag, .. })) =
        (&n.operation, c.value(selector))
    else {
        c.fail(RuntimeError::InvalidProgram);
        return u64::MAX;
    };
    arms.iter()
        .position(|a| &a.tag == tag)
        .unwrap_or(arms.len()) as u64
}
/// # Safety
/// Context is live and exclusive.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn g0_native_tick(context: *mut NativeContext) -> u64 {
    let Some(c) = (unsafe { context.as_mut() }) else {
        return 0;
    };
    match c.tick() {
        Ok(()) => 1,
        Err(e) => c.fail(e),
    }
}
