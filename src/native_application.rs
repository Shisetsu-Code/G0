//! A graph-native service boundary: certificate identity -> scoped principal ->
//! typed graph -> explicit storage transaction -> the same typed wire value.
use crate::{
    authority::Principal,
    execution::{ExecutionLimits, Executor, RuntimeError},
    gir::Capability,
    native_transport::{SecureListener, TransportError},
    program::ProgramContract,
    storage_host::StorageHost,
    store_engine::{NativeStore, StoreError},
    value_codec::validate_public_type,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Debug)]
pub enum ApplicationError {
    InvalidInterface,
    Denied,
    Runtime(RuntimeError),
    Transport(TransportError),
    Store(StoreError),
}
#[derive(Debug)]
pub struct ServiceOutcome {
    pub graph: String,
    pub steps: u64,
}
pub struct GraphService {
    program: Arc<ProgramContract>,
    graph: String,
    principals: BTreeMap<[u8; 32], Principal>,
    grants: BTreeSet<Capability>,
    limits: ExecutionLimits,
}
impl GraphService {
    pub fn new(
        program: Arc<ProgramContract>,
        graph: String,
        principals: BTreeMap<[u8; 32], Principal>,
        grants: BTreeSet<Capability>,
        limits: ExecutionLimits,
    ) -> Result<Self, ApplicationError> {
        Executor::new(&program, limits).map_err(ApplicationError::Runtime)?;
        let entry = program
            .graphs
            .iter()
            .find(|g| g.name == graph)
            .ok_or(ApplicationError::InvalidInterface)?;
        if entry.inputs.len() != 1 || entry.outputs.len() != 1 {
            return Err(ApplicationError::InvalidInterface);
        }
        validate_public_type(&entry.inputs[0].ty, &program.schemas)
            .map_err(|_| ApplicationError::InvalidInterface)?;
        validate_public_type(&entry.outputs[0].ty, &program.schemas)
            .map_err(|_| ApplicationError::InvalidInterface)?;
        Ok(Self {
            program,
            graph,
            principals,
            grants,
            limits,
        })
    }
    pub fn serve_one(
        &self,
        listener: &SecureListener,
        store: &mut NativeStore,
    ) -> Result<ServiceOutcome, ApplicationError> {
        let mut channel = listener.accept().map_err(ApplicationError::Transport)?;
        let principal = self
            .principals
            .get(&channel.peer_identity().certificate_sha256)
            .ok_or(ApplicationError::Denied)?;
        let entry = self
            .program
            .graphs
            .iter()
            .find(|g| g.name == self.graph)
            .unwrap();
        let value = channel
            .receive(&entry.inputs[0].ty, &self.program.schemas)
            .map_err(ApplicationError::Transport)?;
        let mut host = StorageHost::new(store, principal.clone());
        let mut runtime =
            Executor::new(&self.program, self.limits).map_err(ApplicationError::Runtime)?;
        for grant in &self.grants {
            runtime.grant(grant.clone());
        }
        let mut outputs = runtime
            .run_with_host(&self.graph, vec![value], &mut host)
            .map_err(ApplicationError::Runtime)?;
        let output = outputs.pop().ok_or(ApplicationError::InvalidInterface)?;
        // Durability before acknowledgment. A failed send does not retry or
        // undo a committed mutation; callers must handle that outcome explicitly.
        host.commit().map_err(ApplicationError::Store)?;
        channel
            .send(&output, &entry.outputs[0].ty, &self.program.schemas)
            .map_err(ApplicationError::Transport)?;
        Ok(ServiceOutcome {
            graph: self.graph.clone(),
            steps: runtime.steps_used(),
        })
    }
}
