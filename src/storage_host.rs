//! GIR effects participate in one explicit host transaction. Execution failure
//! drops the staged transaction; only the caller can commit after valid outputs.
use crate::{
    authority::Principal,
    execution::{EffectHost, RuntimeError},
    gir::{Capability, CapabilityClass, Node, Operation, SemanticType},
    store_engine::{NativeStore, StoreError, Transaction},
    value::Value,
};
use std::{collections::BTreeMap, sync::Arc};

pub struct StorageHost<'a> {
    store: &'a mut NativeStore,
    transaction: Transaction,
    scope: String,
}
impl<'a> StorageHost<'a> {
    pub fn new(store: &'a mut NativeStore, principal: Principal) -> Self {
        let scope = principal.scope.0.clone();
        let transaction = store.begin(principal);
        Self {
            store,
            transaction,
            scope,
        }
    }
    pub fn commit(self) -> Result<(), StoreError> {
        self.store.commit(self.transaction)
    }
    fn failure(node: &Node, error: StoreError) -> RuntimeError {
        let code = match error {
            StoreError::Denied => "denied",
            StoreError::NotFound => "not-found",
            StoreError::Conflict => "conflict",
            StoreError::AlreadyExists => "already-exists",
            StoreError::TypeMismatch => "type",
            StoreError::AuthorityChanged => "authority-changed",
            _ => "storage-failed",
        };
        RuntimeError::EffectFailure {
            node: node.id,
            code,
        }
    }
}
impl EffectHost for StorageHost<'_> {
    fn execute(&mut self, node: &Node, inputs: &[Value]) -> Result<Vec<Value>, RuntimeError> {
        let (action, resource) = match &node.operation {
            Operation::StoreRead { resource, .. } => ("read", resource),
            Operation::StoreCreate { resource, .. } => ("create", resource),
            Operation::StoreUpdate { resource, .. } => ("update", resource),
            Operation::StoreDelete { resource } => ("delete", resource),
            Operation::StoreEnumerate { resource } => ("enumerate", resource),
            _ => {
                return Err(RuntimeError::Unsupported {
                    graph: String::new(),
                    node: node.id,
                });
            }
        };
        let capability = Capability::new(CapabilityClass::Storage, action, resource, &self.scope);
        if !node.required_capabilities.contains(&capability) {
            return Err(RuntimeError::MissingCapability(capability));
        }
        let bad = || RuntimeError::InvalidHostResult;
        let id = || match inputs.first() {
            Some(Value::Text(id)) => Ok(id.as_ref()),
            _ => Err(bad()),
        };
        let failure = |error| Self::failure(node, error);
        match &node.operation {
            Operation::StoreRead { fields, .. } => {
                if !(1..=2).contains(&inputs.len()) {
                    return Err(bad());
                }
                // An optional second input is the explicit mutation version edge
                // that orders this read after the mutation in the graph.
                if inputs.len() == 2 && !matches!(inputs[1], Value::Integer(_)) {
                    return Err(bad());
                }
                if fields
                    .iter()
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != fields.len()
                {
                    return Err(bad());
                }
                if node.outputs.len() == 1
                    && node.outputs[0].ty == SemanticType::Record(resource.clone())
                {
                    let all = self.store.resource_fields(resource).map_err(failure)?;
                    if all.iter().collect::<std::collections::BTreeSet<_>>()
                        != fields.iter().collect()
                    {
                        return Err(bad());
                    }
                    let stored = self
                        .store
                        .read(&self.transaction, resource, id()?)
                        .map_err(failure)?;
                    Ok(vec![stored.value])
                } else {
                    self.store
                        .read_fields(&self.transaction, resource, id()?, fields)
                        .map_err(failure)
                }
            }
            Operation::StoreCreate { fields, .. } => {
                if inputs.len() != 2 {
                    return Err(bad());
                }
                let Value::Record {
                    schema,
                    fields: values,
                } = &inputs[1]
                else {
                    return Err(bad());
                };
                if schema != resource
                    || fields
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len()
                        != fields.len()
                    || values.keys().collect::<std::collections::BTreeSet<_>>()
                        != fields.iter().collect()
                {
                    return Err(bad());
                }
                self.store
                    .create(&mut self.transaction, id()?, inputs[1].clone())
                    .map_err(failure)?;
                // Reading for the version would add a hidden read permission.
                Ok(vec![Value::Integer(i128::from(
                    self.transaction.prospective_version().ok_or_else(bad)?,
                ))])
            }
            Operation::StoreUpdate { fields, .. } => {
                if inputs.len() != fields.len() + 1 {
                    return Err(bad());
                }
                let changes: BTreeMap<_, _> = fields
                    .iter()
                    .cloned()
                    .zip(inputs[1..].iter().cloned())
                    .collect();
                if changes.len() != fields.len() {
                    return Err(bad());
                }
                self.store
                    .update(&mut self.transaction, resource, id()?, changes)
                    .map_err(failure)?;
                Ok(vec![Value::Integer(i128::from(
                    self.transaction.prospective_version().ok_or_else(bad)?,
                ))])
            }
            Operation::StoreDelete { .. } => {
                if inputs.len() != 1 {
                    return Err(bad());
                }
                self.store
                    .delete(&mut self.transaction, resource, id()?)
                    .map_err(failure)?;
                Ok(vec![Value::Bool(true)])
            }
            Operation::StoreEnumerate { .. } => {
                if !inputs.is_empty() {
                    return Err(bad());
                }
                let values = self
                    .store
                    .enumerate(&self.transaction, resource)
                    .map_err(failure)?
                    .into_iter()
                    .map(|v| v.value)
                    .collect::<Vec<_>>();
                Ok(vec![Value::Array(Arc::from(values))])
            }
            _ => Err(bad()),
        }
    }
}
