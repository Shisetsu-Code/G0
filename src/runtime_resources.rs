//! Executable region lifetimes and structured task scopes for the native host.
use crate::{
    execution::{Cancellation, ExecutionLimits, Executor, RuntimeError},
    gir::Capability,
    program::ProgramContract,
    value::Value,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Deref,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
};

static ARENA_ID: AtomicU64 = AtomicU64::new(1);
pub(crate) fn fresh_identity(counter: &AtomicU64) -> Option<u64> {
    let mut current = counter.load(Ordering::Relaxed);
    loop {
        let next = current.checked_add(1)?;
        match counter.compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(previous) => return Some(previous),
            Err(actual) => current = actual,
        }
    }
}
pub type RegionId = u64;
#[derive(Debug)]
pub struct RegionHandle {
    arena: u64,
    slot: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionError {
    Budget,
    Closed,
    UnknownRegion,
    ForeignHandle,
    Bounds,
    SecretExport,
    Limit,
}
pub struct RegionBorrow<'a> {
    bytes: &'a [u8],
    secret: bool,
}
impl std::fmt::Debug for RegionBorrow<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.secret {
            f.write_str("RegionBorrow(<redacted>)")
        } else {
            self.bytes.fmt(f)
        }
    }
}
impl Deref for RegionBorrow<'_> {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.bytes
    }
}

struct Region {
    parent: Option<RegionId>,
    max_bytes: usize,
    bytes: usize,
    secret: bool,
    closed: bool,
}
struct Buffer {
    region: RegionId,
    bytes: Vec<u8>,
    secret: bool,
}
impl Drop for Buffer {
    fn drop(&mut self) {
        if self.secret {
            wipe(&mut self.bytes);
        }
    }
}
fn wipe(bytes: &mut [u8]) {
    for byte in bytes {
        // Each pointer comes from a live unique mutable reference. Volatile
        // stores plus the fence prevent dead-store elimination of the wipe.
        unsafe {
            std::ptr::write_volatile(byte, 0);
        }
    }
    std::sync::atomic::compiler_fence(Ordering::SeqCst);
}

pub struct RegionArena {
    id: u64,
    max_bytes: usize,
    bytes: usize,
    next_region: u64,
    next_slot: u64,
    regions: BTreeMap<RegionId, Region>,
    buffers: BTreeMap<u64, Buffer>,
    closed_slots: BTreeSet<u64>,
}
impl RegionArena {
    pub fn new(max_bytes: usize) -> Result<Self, RegionError> {
        let id = fresh_identity(&ARENA_ID).ok_or(RegionError::Limit)?;
        Ok(Self {
            id,
            max_bytes,
            bytes: 0,
            next_region: 0,
            next_slot: 0,
            regions: BTreeMap::new(),
            buffers: BTreeMap::new(),
            closed_slots: BTreeSet::new(),
        })
    }
    pub fn bytes_used(&self) -> usize {
        self.bytes
    }
    pub fn create_region(
        &mut self,
        parent: Option<RegionId>,
        max_bytes: usize,
        secret: bool,
    ) -> Result<RegionId, RegionError> {
        if self.regions.len() >= 4096 || max_bytes > self.max_bytes {
            return Err(RegionError::Budget);
        }
        let secret = if let Some(parent) = parent {
            let parent = self.region(parent)?;
            if max_bytes > parent.max_bytes {
                return Err(RegionError::Budget);
            }
            secret || parent.secret
        } else {
            secret
        };
        let id = self.next_region;
        self.next_region = id.checked_add(1).ok_or(RegionError::Limit)?;
        self.regions.insert(
            id,
            Region {
                parent,
                max_bytes,
                bytes: 0,
                secret,
                closed: false,
            },
        );
        Ok(id)
    }
    fn region(&self, id: RegionId) -> Result<&Region, RegionError> {
        let region = self.regions.get(&id).ok_or(RegionError::UnknownRegion)?;
        if region.closed {
            return Err(RegionError::Closed);
        }
        Ok(region)
    }
    fn ancestors(&self, id: RegionId) -> Result<Vec<RegionId>, RegionError> {
        let mut result = Vec::new();
        let mut current = Some(id);
        while let Some(id) = current {
            result.push(id);
            current = self.region(id)?.parent;
        }
        Ok(result)
    }
    pub fn is_secret(&self, id: RegionId) -> Result<bool, RegionError> {
        Ok(self.region(id)?.secret)
    }
    fn check_allocation(
        &self,
        region: RegionId,
        bytes: usize,
    ) -> Result<Vec<RegionId>, RegionError> {
        if self.next_slot >= 1_000_000
            || self
                .bytes
                .checked_add(bytes)
                .is_none_or(|v| v > self.max_bytes)
        {
            return Err(RegionError::Budget);
        }
        let ancestors = self.ancestors(region)?;
        for id in &ancestors {
            let r = &self.regions[id];
            if r.bytes.checked_add(bytes).is_none_or(|v| v > r.max_bytes) {
                return Err(RegionError::Budget);
            }
        }
        Ok(ancestors)
    }
    pub fn allocate_zeroed(
        &mut self,
        region: RegionId,
        size: usize,
    ) -> Result<RegionHandle, RegionError> {
        self.check_allocation(region, size)?;
        self.allocate(region, vec![0; size])
    }
    pub fn allocate(
        &mut self,
        region: RegionId,
        bytes: Vec<u8>,
    ) -> Result<RegionHandle, RegionError> {
        let ancestors = self.check_allocation(region, bytes.len())?;
        let slot = self.next_slot;
        self.next_slot += 1;
        self.bytes += bytes.len();
        for id in ancestors {
            self.regions.get_mut(&id).unwrap().bytes += bytes.len();
        }
        self.buffers.insert(
            slot,
            Buffer {
                region,
                secret: self.regions[&region].secret,
                bytes,
            },
        );
        Ok(RegionHandle {
            arena: self.id,
            slot,
        })
    }
    fn buffer(&self, handle: &RegionHandle) -> Result<&Buffer, RegionError> {
        if handle.arena != self.id {
            return Err(RegionError::ForeignHandle);
        }
        self.buffers
            .get(&handle.slot)
            .ok_or(if self.closed_slots.contains(&handle.slot) {
                RegionError::Closed
            } else {
                RegionError::Bounds
            })
    }
    pub fn borrow(&self, handle: &RegionHandle) -> Result<RegionBorrow<'_>, RegionError> {
        let buffer = self.buffer(handle)?;
        Ok(RegionBorrow {
            bytes: &buffer.bytes,
            secret: buffer.secret,
        })
    }
    pub fn write(
        &mut self,
        handle: &RegionHandle,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), RegionError> {
        let buffer = self.buffer(handle)?;
        let end = offset
            .checked_add(bytes.len())
            .filter(|end| *end <= buffer.bytes.len())
            .ok_or(RegionError::Bounds)?;
        self.buffers.get_mut(&handle.slot).unwrap().bytes[offset..end].copy_from_slice(bytes);
        Ok(())
    }
    pub fn take(&mut self, handle: RegionHandle) -> Result<Vec<u8>, RegionError> {
        if self.buffer(&handle)?.secret {
            return Err(RegionError::SecretExport);
        }
        let mut buffer = self.buffers.remove(&handle.slot).unwrap();
        self.release_bytes(buffer.region, buffer.bytes.len());
        self.closed_slots.insert(handle.slot);
        Ok(std::mem::take(&mut buffer.bytes))
    }
    fn release_bytes(&mut self, region: RegionId, bytes: usize) {
        self.bytes -= bytes;
        let mut current = Some(region);
        while let Some(id) = current {
            let r = self.regions.get_mut(&id).unwrap();
            r.bytes -= bytes;
            current = r.parent;
        }
    }
    pub fn close_region(&mut self, region: RegionId) -> Result<(), RegionError> {
        self.region(region)?;
        let mut closed = BTreeSet::from([region]);
        loop {
            let previous = closed.len();
            for (id, r) in &self.regions {
                if r.parent.is_some_and(|p| closed.contains(&p)) {
                    closed.insert(*id);
                }
            }
            if closed.len() == previous {
                break;
            }
        }
        let slots: Vec<_> = self
            .buffers
            .iter()
            .filter_map(|(slot, b)| closed.contains(&b.region).then_some(*slot))
            .collect();
        for slot in slots {
            let buffer = self.buffers.remove(&slot).unwrap();
            self.release_bytes(buffer.region, buffer.bytes.len());
            self.closed_slots.insert(slot);
        }
        for id in closed {
            self.regions.get_mut(&id).unwrap().closed = true;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum TaskError {
    Authority,
    Budget,
    Cancelled,
    UnknownTask,
    Panic,
    Spawn(std::io::Error),
    Runtime(RuntimeError),
}
pub type TaskId = u64;
pub struct TaskGroup {
    limits: ExecutionLimits,
    steps_reserved: u64,
    bytes_reserved: u64,
    grants: BTreeSet<Capability>,
    cancel: Cancellation,
    next: TaskId,
    tasks: BTreeMap<TaskId, JoinHandle<Result<Vec<Value>, RuntimeError>>>,
}
impl TaskGroup {
    pub fn new(limits: ExecutionLimits, grants: BTreeSet<Capability>) -> Self {
        Self {
            limits,
            steps_reserved: 0,
            bytes_reserved: 0,
            grants,
            cancel: Cancellation::default(),
            next: 0,
            tasks: BTreeMap::new(),
        }
    }
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    pub fn cancellation(&self) -> Cancellation {
        self.cancel.clone()
    }
    pub fn spawn(
        &mut self,
        program: Arc<ProgramContract>,
        graph: String,
        inputs: Vec<Value>,
        grants: BTreeSet<Capability>,
        limits: ExecutionLimits,
    ) -> Result<TaskId, TaskError> {
        if self.cancel.is_cancelled() {
            return Err(TaskError::Cancelled);
        }
        if !grants.is_subset(&self.grants) {
            return Err(TaskError::Authority);
        }
        let steps = self
            .steps_reserved
            .checked_add(limits.max_steps)
            .filter(|v| *v <= self.limits.max_steps)
            .ok_or(TaskError::Budget)?;
        let bytes = self
            .bytes_reserved
            .checked_add(limits.max_value_bytes)
            .filter(|v| *v <= self.limits.max_value_bytes)
            .ok_or(TaskError::Budget)?;
        if limits.max_call_depth > self.limits.max_call_depth || self.next >= 4096 {
            return Err(TaskError::Budget);
        }
        let cancel = self.cancel.clone();
        let task = thread::Builder::new()
            .name(format!("g0-task-{}", self.next))
            .spawn(move || {
                let mut runtime = Executor::new(&program, limits)?;
                runtime.set_cancellation(cancel);
                for grant in grants {
                    runtime.grant(grant);
                }
                runtime.run_graph(&graph, inputs)
            })
            .map_err(TaskError::Spawn)?;
        self.steps_reserved = steps;
        self.bytes_reserved = bytes;
        let id = self.next;
        self.next += 1;
        self.tasks.insert(id, task);
        Ok(id)
    }
    pub fn join(&mut self, id: TaskId) -> Result<Vec<Value>, TaskError> {
        self.tasks
            .remove(&id)
            .ok_or(TaskError::UnknownTask)?
            .join()
            .map_err(|_| TaskError::Panic)?
            .map_err(TaskError::Runtime)
    }
}
impl Drop for TaskGroup {
    fn drop(&mut self) {
        self.cancel();
        for (_, task) in std::mem::take(&mut self.tasks) {
            let _ = task.join();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn secret_wipe_overwrites_all_bytes() {
        let mut bytes = vec![42; 1024];
        super::wipe(&mut bytes);
        assert!(bytes.iter().all(|v| *v == 0));
    }
}
