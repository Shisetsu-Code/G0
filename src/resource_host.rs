//! Linear opaque GIR handles. They can be moved but not minted by literals,
//! serialized, transferred to a foreign host or reused after consumption.
use crate::{
    execution::{EffectHost, ExecutionLimits, Executor, RuntimeError},
    gir::*,
    program::ProgramContract,
    runtime_resources::{RegionArena, RegionHandle, RegionId, TaskGroup, TaskId, fresh_identity},
    value::Value,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, atomic::AtomicU64},
};

static HOST_ID: AtomicU64 = AtomicU64::new(1);
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NativeHandle {
    issuer: u64,
    token: u64,
    kind: Arc<str>,
}
impl NativeHandle {
    pub fn kind(&self) -> &str {
        &self.kind
    }
}
impl std::fmt::Debug for NativeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NativeHandle({})", self.kind)
    }
}
enum Resource {
    Region(RegionId),
    Buffer {
        region: RegionId,
        handle: RegionHandle,
    },
    Task {
        body: String,
        id: TaskId,
    },
}
pub struct ResourceHost {
    id: u64,
    next: u64,
    arena: RegionArena,
    resources: BTreeMap<u64, Resource>,
    program: Arc<ProgramContract>,
    limits: ExecutionLimits,
    grants: BTreeSet<Capability>,
    tasks: TaskGroup,
    charged: u64,
}
impl ResourceHost {
    pub fn new(
        program: Arc<ProgramContract>,
        limits: ExecutionLimits,
        grants: BTreeSet<Capability>,
    ) -> Result<Self, RuntimeError> {
        Executor::new(&program, limits)?;
        let id = fresh_identity(&HOST_ID).ok_or(RuntimeError::MemoryLimit)?;
        let max = usize::try_from(limits.max_value_bytes).map_err(|_| RuntimeError::MemoryLimit)?;
        let arena = RegionArena::new(max).map_err(|_| RuntimeError::MemoryLimit)?;
        let tasks = TaskGroup::new(limits, grants.clone());
        Ok(Self {
            id,
            next: 0,
            arena,
            resources: BTreeMap::new(),
            program,
            limits,
            grants,
            tasks,
            charged: 0,
        })
    }
    pub fn run_graph(
        &mut self,
        name: &str,
        inputs: Vec<Value>,
    ) -> Result<Vec<Value>, RuntimeError> {
        let program = self.program.clone();
        let mut executor = Executor::new(&program, self.limits)?;
        executor.set_cancellation(self.tasks.cancellation());
        for grant in &self.grants {
            executor.grant(grant.clone());
        }
        executor.run_with_host(name, inputs, self)
    }
    pub fn cancellation(&self) -> crate::execution::Cancellation {
        self.tasks.cancellation()
    }
    fn charge(&mut self, bytes: u64) -> Result<(), RuntimeError> {
        self.charged = self
            .charged
            .checked_add(bytes)
            .filter(|n| *n <= self.limits.max_value_bytes)
            .ok_or(RuntimeError::MemoryLimit)?;
        Ok(())
    }
    fn issue(&mut self, resource: Resource, kind: &str) -> Value {
        let token = self.next;
        self.next += 1;
        self.resources.insert(token, resource);
        Value::NativeHandle(NativeHandle {
            issuer: self.id,
            token,
            kind: kind.into(),
        })
    }
    fn take(&mut self, value: &Value, kind: &str, node: &Node) -> Result<Resource, RuntimeError> {
        let Value::NativeHandle(handle) = value else {
            return Err(failure(node, "invalid-resource-handle"));
        };
        if handle.issuer != self.id || handle.kind.as_ref() != kind {
            return Err(failure(node, "foreign-resource-handle"));
        }
        self.resources
            .remove(&handle.token)
            .ok_or_else(|| failure(node, "consumed-resource-handle"))
    }
    fn region(&mut self, value: &Value, node: &Node) -> Result<RegionId, RuntimeError> {
        let kind = match value {
            Value::NativeHandle(h) if h.kind() == "g0.region" => "g0.region",
            Value::NativeHandle(h) if h.kind() == "g0.secret-region" => "g0.secret-region",
            _ => return Err(failure(node, "invalid-region")),
        };
        match self.take(value, kind, node)? {
            Resource::Region(id) => Ok(id),
            _ => Err(failure(node, "invalid-region")),
        }
    }
    fn buffer(
        &mut self,
        value: &Value,
        region: RegionId,
        node: &Node,
    ) -> Result<RegionHandle, RuntimeError> {
        let kind = if self
            .arena
            .is_secret(region)
            .map_err(|_| failure(node, "region-closed"))?
        {
            "g0.secret-buffer"
        } else {
            "g0.buffer"
        };
        match self.take(value, kind, node)? {
            Resource::Buffer {
                region: owner,
                handle,
            } if owner == region => Ok(handle),
            _ => Err(failure(node, "foreign-buffer-region")),
        }
    }
}
fn task_failure(node: &Node, error: crate::runtime_resources::TaskError) -> RuntimeError {
    use crate::runtime_resources::TaskError;
    match error {
        TaskError::Runtime(error) => error,
        TaskError::Cancelled => RuntimeError::Cancelled,
        TaskError::Panic => failure(node, "task-panicked"),
        _ => failure(node, "task-budget-or-invalid-handle"),
    }
}
fn failure(node: &Node, code: &'static str) -> RuntimeError {
    RuntimeError::EffectFailure {
        node: node.id,
        code,
    }
}
pub fn validate_node(node: &Node) -> Result<(), String> {
    let handle = |kind: &str| SemanticType::Unique(Box::new(SemanticType::Reference(kind.into())));
    let secret_region = handle("g0.secret-region");
    let mut inputs: Vec<_> = node.inputs.iter().collect();
    inputs.sort_by_key(|p| p.id);
    let mut outputs: Vec<_> = node.outputs.iter().collect();
    outputs.sort_by_key(|p| p.id);
    let inputs: Vec<_> = inputs.into_iter().map(|p| p.ty.clone()).collect();
    let outputs: Vec<_> = outputs.into_iter().map(|p| p.ty.clone()).collect();
    let secret = inputs.first() == Some(&secret_region);
    let region = if secret {
        secret_region.clone()
    } else {
        handle("g0.region")
    };
    let buffer = if secret {
        handle("g0.secret-buffer")
    } else {
        handle("g0.buffer")
    };
    let bytes = if secret {
        SemanticType::Secret(Box::new(SemanticType::Bytes))
    } else {
        SemanticType::Bytes
    };
    let size = |ty: &SemanticType| matches!(ty,SemanticType::Integer(t) if t.min>=0);
    let valid = match &node.operation {
        Operation::RegionOpen => {
            inputs.len() == 1 && size(&inputs[0]) && outputs == vec![region.clone()]
        }
        Operation::RegionOpenSecret => {
            inputs.len() == 1 && size(&inputs[0]) && outputs == vec![secret_region.clone()]
        }
        Operation::RegionOpenChild { secret: requested } => {
            inputs.len() == 2
                && inputs[0] == region
                && size(&inputs[1])
                && outputs
                    == vec![
                        region.clone(),
                        if secret || *requested {
                            secret_region
                        } else {
                            region.clone()
                        },
                    ]
        }
        Operation::RegionAllocate => {
            inputs.len() == 2
                && inputs[0] == region
                && size(&inputs[1])
                && outputs == vec![region.clone(), buffer.clone()]
        }
        Operation::RegionWrite => {
            inputs.len() == 4
                && inputs[0] == region
                && inputs[1] == buffer
                && size(&inputs[2])
                && inputs[3] == bytes
                && outputs == vec![region.clone(), buffer.clone()]
        }
        Operation::RegionRead => {
            inputs == vec![region.clone(), buffer.clone()]
                && outputs == vec![region.clone(), buffer.clone(), bytes]
        }
        Operation::RegionClose => {
            (inputs == vec![region.clone()] || inputs == vec![region, SemanticType::Bool])
                && outputs == vec![SemanticType::Bool]
        }
        _ => return Ok(()),
    };
    if !valid
        || node.effects != BTreeSet::from([Effect::MemoryWrite])
        || !node.required_capabilities.is_empty()
    {
        Err("region operation requires typed linear handles, nonnegative sizes and MemoryWrite effect".into())
    } else {
        Ok(())
    }
}
fn size(value: &Value, node: &Node) -> Result<usize, RuntimeError> {
    match value {
        Value::Integer(n) => usize::try_from(*n).map_err(|_| failure(node, "invalid-region-size")),
        _ => Err(failure(node, "invalid-region-size")),
    }
}
pub struct ChainedHost<'a> {
    pub resources: &'a mut ResourceHost,
    pub fallback: &'a mut dyn EffectHost,
}
impl EffectHost for ChainedHost<'_> {
    fn execute(&mut self, node: &Node, inputs: &[Value]) -> Result<Vec<Value>, RuntimeError> {
        match node.operation {
            Operation::RegionOpen
            | Operation::RegionOpenSecret
            | Operation::RegionOpenChild { .. }
            | Operation::RegionAllocate
            | Operation::RegionWrite
            | Operation::RegionRead
            | Operation::RegionClose
            | Operation::TaskSpawn { .. }
            | Operation::TaskJoin { .. }
            | Operation::TaskSpawnScoped { .. }
            | Operation::TaskJoinScoped { .. } => self.resources.execute(node, inputs),
            _ => self.fallback.execute(node, inputs),
        }
    }
}
impl EffectHost for ResourceHost {
    fn execute(&mut self, node: &Node, args: &[Value]) -> Result<Vec<Value>, RuntimeError> {
        if self.tasks.cancellation().is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        if self.next > 16380 {
            return Err(failure(node, "resource-handle-limit"));
        }
        self.charge(1024)?;
        match (&node.operation, args) {
            (
                Operation::TaskSpawn {
                    body,
                    max_steps,
                    max_value_bytes,
                }
                | Operation::TaskSpawnScoped {
                    body,
                    max_steps,
                    max_value_bytes,
                },
                inputs,
            ) => {
                let scoped = matches!(node.operation, Operation::TaskSpawnScoped { .. });
                let action = if scoped { "spawn-scoped" } else { "spawn" };
                let kind = if scoped { "g0.scoped-task" } else { "g0.task" };
                let cap = Capability::new(CapabilityClass::LocalExecution, action, body, "tasks");
                if !self.grants.contains(&cap) || !node.required_capabilities.contains(&cap) {
                    return Err(RuntimeError::MissingCapability(cap));
                }
                self.charge(
                    max_value_bytes
                        .checked_mul(if scoped { 2 } else { 1 })
                        .ok_or(RuntimeError::MemoryLimit)?,
                )?;
                let arity = self
                    .program
                    .graphs
                    .iter()
                    .find(|g| &g.name == body)
                    .ok_or_else(|| failure(node, "unknown-task-body"))?
                    .inputs
                    .len();
                if inputs.len() < arity
                    || inputs.len() > arity + 1
                    || (inputs.len() > arity && !matches!(inputs.last(), Some(Value::Bool(_))))
                {
                    return Err(failure(node, "invalid-task-inputs"));
                }
                let limits = ExecutionLimits {
                    max_steps: *max_steps,
                    max_value_bytes: *max_value_bytes,
                    max_call_depth: self.limits.max_call_depth,
                };
                let id = if scoped {
                    self.tasks.spawn_scoped(
                        self.program.clone(),
                        body.clone(),
                        inputs[..arity].to_vec(),
                        limits,
                    )
                } else {
                    self.tasks.spawn(
                        self.program.clone(),
                        body.clone(),
                        inputs[..arity].to_vec(),
                        BTreeSet::new(),
                        limits,
                    )
                }
                .map_err(|_| failure(node, "task-budget-or-cancelled"))?;
                let mut outputs = vec![self.issue(
                    Resource::Task {
                        body: body.clone(),
                        id,
                    },
                    &format!("{kind}:{body}"),
                )];
                if node.outputs.len() == 2 {
                    outputs.push(Value::Bool(true));
                }
                Ok(outputs)
            }
            (Operation::TaskJoin { body } | Operation::TaskJoinScoped { body }, inputs) => {
                let scoped = matches!(node.operation, Operation::TaskJoinScoped { .. });
                let action = if scoped { "join-scoped" } else { "join" };
                let kind = if scoped { "g0.scoped-task" } else { "g0.task" };
                let cap = Capability::new(CapabilityClass::LocalExecution, action, body, "tasks");
                if !self.grants.contains(&cap) || !node.required_capabilities.contains(&cap) {
                    return Err(RuntimeError::MissingCapability(cap));
                }
                if inputs.is_empty()
                    || inputs.len() > 2
                    || (inputs.len() == 2 && !matches!(inputs[1], Value::Bool(_)))
                {
                    return Err(failure(node, "invalid-task-inputs"));
                }
                match self.take(&inputs[0], &format!("{kind}:{body}"), node)? {
                    Resource::Task { body: actual, id } if actual == *body => {
                        let mut outputs = self
                            .tasks
                            .join(id)
                            .map_err(|error| task_failure(node, error))?;
                        if node.outputs.len() == outputs.len() + 1 {
                            outputs.push(Value::Bool(true));
                        }
                        Ok(outputs)
                    }
                    _ => Err(failure(node, "invalid-task")),
                }
            }
            (Operation::RegionOpen | Operation::RegionOpenSecret, [limit]) => {
                let secret = matches!(node.operation, Operation::RegionOpenSecret);
                let region = self
                    .arena
                    .create_region(None, size(limit, node)?, secret)
                    .map_err(|_| failure(node, "region-budget"))?;
                Ok(vec![self.issue(
                    Resource::Region(region),
                    if secret {
                        "g0.secret-region"
                    } else {
                        "g0.region"
                    },
                )])
            }
            (Operation::RegionOpenChild { secret }, [parent, limit]) => {
                let limit = size(limit, node)?;
                let parent = self.region(parent, node)?;
                let parent_secret = self
                    .arena
                    .is_secret(parent)
                    .map_err(|_| failure(node, "region-closed"))?;
                let child = self
                    .arena
                    .create_region(Some(parent), limit, *secret)
                    .map_err(|_| failure(node, "region-budget-or-closed"))?;
                Ok(vec![
                    self.issue(
                        Resource::Region(parent),
                        if parent_secret {
                            "g0.secret-region"
                        } else {
                            "g0.region"
                        },
                    ),
                    self.issue(
                        Resource::Region(child),
                        if parent_secret || *secret {
                            "g0.secret-region"
                        } else {
                            "g0.region"
                        },
                    ),
                ])
            }
            (Operation::RegionAllocate, [region, length]) => {
                let length = size(length, node)?;
                self.charge(length as u64)?;
                let region = self.region(region, node)?;
                let secret = self
                    .arena
                    .is_secret(region)
                    .map_err(|_| failure(node, "region-closed"))?;
                let handle = self
                    .arena
                    .allocate_zeroed(region, length)
                    .map_err(|_| failure(node, "region-budget-or-closed"))?;
                Ok(vec![
                    self.issue(
                        Resource::Region(region),
                        if secret {
                            "g0.secret-region"
                        } else {
                            "g0.region"
                        },
                    ),
                    self.issue(
                        Resource::Buffer { region, handle },
                        if secret {
                            "g0.secret-buffer"
                        } else {
                            "g0.buffer"
                        },
                    ),
                ])
            }
            (Operation::RegionWrite, [region, buffer, offset, data]) => {
                let offset = size(offset, node)?;
                let region = self.region(region, node)?;
                let secret = self
                    .arena
                    .is_secret(region)
                    .map_err(|_| failure(node, "region-closed"))?;
                let bytes = match (secret, data) {
                    (false, Value::Bytes(bytes)) => bytes,
                    (true, Value::Secret(data)) => match data.as_ref() {
                        Value::Bytes(bytes) => bytes,
                        _ => return Err(failure(node, "secret-bytes-required")),
                    },
                    _ => return Err(failure(node, "secret-bytes-required")),
                };
                let handle = self.buffer(buffer, region, node)?;
                self.arena
                    .write(&handle, offset, bytes)
                    .map_err(|_| failure(node, "region-bounds-or-closed"))?;
                Ok(vec![
                    self.issue(
                        Resource::Region(region),
                        if secret {
                            "g0.secret-region"
                        } else {
                            "g0.region"
                        },
                    ),
                    self.issue(
                        Resource::Buffer { region, handle },
                        if secret {
                            "g0.secret-buffer"
                        } else {
                            "g0.buffer"
                        },
                    ),
                ])
            }
            (Operation::RegionRead, [region, buffer]) => {
                let region = self.region(region, node)?;
                let secret = self
                    .arena
                    .is_secret(region)
                    .map_err(|_| failure(node, "region-closed"))?;
                let handle = self.buffer(buffer, region, node)?;
                let length = self
                    .arena
                    .borrow(&handle)
                    .map_err(|_| failure(node, "region-closed"))?
                    .len();
                self.charge(length as u64)?;
                let bytes = Value::Bytes(Arc::from(
                    self.arena
                        .borrow(&handle)
                        .map_err(|_| failure(node, "region-closed"))?
                        .as_ref(),
                ));
                Ok(vec![
                    self.issue(
                        Resource::Region(region),
                        if secret {
                            "g0.secret-region"
                        } else {
                            "g0.region"
                        },
                    ),
                    self.issue(
                        Resource::Buffer { region, handle },
                        if secret {
                            "g0.secret-buffer"
                        } else {
                            "g0.buffer"
                        },
                    ),
                    if secret {
                        Value::Secret(Arc::new(bytes))
                    } else {
                        bytes
                    },
                ])
            }
            (Operation::RegionClose, [region] | [region, Value::Bool(_)]) => {
                let region = self.region(region, node)?;
                self.arena
                    .close_region(region)
                    .map_err(|_| failure(node, "region-closed"))?;
                Ok(vec![Value::Bool(true)])
            }
            _ => Err(failure(node, "unsupported-resource-operation")),
        }
    }
}
