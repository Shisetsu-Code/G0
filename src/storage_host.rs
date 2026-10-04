//! GIR effects participate in one explicit host transaction. Execution failure
//! drops the staged transaction; only the caller can commit after valid outputs.
use crate::{
    authority::Principal,
    execution::{EffectHost, RuntimeError},
    gir::{Capability, CapabilityClass, Node, Operation, SemanticType},
    store_engine::{NativeStore, StoreError, Transaction},
    value::Value,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

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
        RuntimeError::EffectFailure {
            node: node.id,
            code: Self::code(error),
        }
    }
    fn code(error: StoreError) -> &'static str {
        match error {
            StoreError::Denied => "denied",
            StoreError::NotFound => "not-found",
            StoreError::Conflict => "conflict",
            StoreError::AlreadyExists => "already-exists",
            StoreError::TypeMismatch => "type",
            StoreError::AuthorityChanged => "authority-changed",
            StoreError::RelationConstraint => "relation-constraint",
            _ => "storage-failed",
        }
    }
}

/// Explicit fixed-principal storage binding for independently committed children.
pub struct StorageTaskFactory {
    store: Arc<Mutex<NativeStore>>,
    principal: Principal,
    allowed: BTreeSet<Capability>,
}
impl StorageTaskFactory {
    pub fn new(
        store: Arc<Mutex<NativeStore>>,
        principal: Principal,
        allowed: BTreeSet<Capability>,
    ) -> Self {
        Self {
            store,
            principal,
            allowed,
        }
    }
}
impl crate::runtime_resources::TaskEffectHostFactory for StorageTaskFactory {
    fn create(
        &self,
        graph: &str,
        grants: &BTreeSet<Capability>,
        cancellation: crate::execution::Cancellation,
    ) -> Result<Box<dyn crate::runtime_resources::TaskEffectHost>, RuntimeError> {
        if cancellation.is_cancelled() {
            return Err(RuntimeError::Cancelled);
        }
        for grant in grants {
            if !self.allowed.contains(grant)
                || grant.class != CapabilityClass::Storage
                || grant.scope != self.principal.scope.0
            {
                return Err(RuntimeError::MissingCapability(grant.clone()));
            }
        }
        let store = self
            .store
            .try_lock()
            .map_err(|_| RuntimeError::HostCompletion {
                graph: graph.into(),
                code: "storage-busy",
            })?;
        let transaction = store.begin(self.principal.clone());
        Ok(Box::new(SharedStorageTaskHost {
            store: self.store.clone(),
            transaction: Some(transaction),
            scope: self.principal.scope.0.clone(),
        }))
    }
}
struct SharedStorageTaskHost {
    store: Arc<Mutex<NativeStore>>,
    transaction: Option<Transaction>,
    scope: String,
}
impl EffectHost for SharedStorageTaskHost {
    fn execute(&mut self, node: &Node, inputs: &[Value]) -> Result<Vec<Value>, RuntimeError> {
        let mut store = self
            .store
            .try_lock()
            .map_err(|_| RuntimeError::EffectFailure {
                node: node.id,
                code: "storage-busy",
            })?;
        let transaction = self
            .transaction
            .take()
            .ok_or(RuntimeError::InvalidHostResult)?;
        let mut borrowed = StorageHost {
            store: &mut store,
            transaction,
            scope: self.scope.clone(),
        };
        let result = borrowed.execute(node, inputs);
        self.transaction = Some(borrowed.transaction);
        result
    }
}
impl crate::runtime_resources::TaskEffectHost for SharedStorageTaskHost {
    fn finish(&mut self) -> Result<(), &'static str> {
        let mut store = self.store.try_lock().map_err(|_| "storage-busy")?;
        store
            .commit(self.transaction.take().ok_or("already-finished")?)
            .map_err(StorageHost::code)
    }
}
impl EffectHost for StorageHost<'_> {
    fn execute(&mut self, node: &Node, inputs: &[Value]) -> Result<Vec<Value>, RuntimeError> {
        let credential_action;
        let (action, resource) = match &node.operation {
            Operation::StoreSetCredential { resource, field } => {
                credential_action = format!("credential:set:{field}");
                (credential_action.as_str(), resource)
            }
            Operation::StoreVerifyCredential { resource, field } => {
                credential_action = format!("credential:verify:{field}");
                (credential_action.as_str(), resource)
            }
            Operation::StoreRead { resource, .. } => ("read", resource),
            Operation::StoreCreate { resource, .. } => ("create", resource),
            Operation::StoreUpdate { resource, .. } => ("update", resource),
            Operation::StoreDelete { resource } => ("delete", resource),
            Operation::StoreEnumerate { resource } => ("enumerate", resource),
            Operation::StoreSetRelation { resource, .. } => ("link", resource),
            Operation::StoreTraverse { resource, .. } => ("traverse", resource),
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
            Operation::StoreSetCredential { field, .. }
            | Operation::StoreVerifyCredential { field, .. } => {
                if !(2..=3).contains(&inputs.len())
                    || (inputs.len() == 3 && !matches!(inputs[2], Value::Integer(_)))
                {
                    return Err(bad());
                }
                if matches!(node.operation, Operation::StoreSetCredential { .. }) {
                    self.store
                        .set_credential(&mut self.transaction, resource, id()?, field, &inputs[1])
                        .map_err(failure)?;
                    Ok(vec![Value::Integer(i128::from(
                        self.transaction.prospective_version().ok_or_else(bad)?,
                    ))])
                } else {
                    Ok(vec![Value::Bool(
                        self.store
                            .verify_credential(
                                &self.transaction,
                                resource,
                                id()?,
                                field,
                                &inputs[1],
                            )
                            .map_err(failure)?,
                    )])
                }
            }
            Operation::StoreSetRelation { relation, .. } => {
                if !(2..=3).contains(&inputs.len())
                    || (inputs.len() == 3 && !matches!(inputs[2], Value::Integer(_)))
                {
                    return Err(bad());
                }
                let (Value::Text(id), Value::Array(ids)) = (&inputs[0], &inputs[1]) else {
                    return Err(bad());
                };
                let ids = ids
                    .iter()
                    .map(|v| match v {
                        Value::Text(id) => Ok(id.to_string()),
                        _ => Err(bad()),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                self.store
                    .set_relation(&mut self.transaction, resource, id, relation, ids)
                    .map_err(failure)?;
                Ok(vec![Value::Integer(i128::from(
                    self.transaction.prospective_version().ok_or_else(bad)?,
                ))])
            }
            Operation::StoreTraverse { relation, .. } => {
                if !(1..=2).contains(&inputs.len())
                    || (inputs.len() == 2 && !matches!(inputs[1], Value::Integer(_)))
                {
                    return Err(bad());
                }
                let Value::Text(id) = &inputs[0] else {
                    return Err(bad());
                };
                let rows = self
                    .store
                    .traverse(&self.transaction, resource, id, relation)
                    .map_err(failure)?;
                Ok(vec![Value::Array(
                    rows.into_iter()
                        .map(|row| row.value)
                        .collect::<Vec<_>>()
                        .into(),
                )])
            }
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
