use crate::conversation::managers::SessionManager;
use crate::session::{CachedMemory, RigStoreConfig};
use std::sync::Arc;

/// The full memory type held by `MemoryState` and shared with the turn
/// executor. `CachedMemory` is cheaply cloneable (its store map is `Arc`-backed),
/// so the `Arc` is what gets handed to each turn's agent.
pub type MemoryOf = Arc<CachedMemory>;

pub struct MemoryState {
    memory: MemoryOf,
}

impl MemoryState {
    pub fn new(rig: RigStoreConfig) -> Self {
        Self {
            memory: Arc::new(CachedMemory::new(rig)),
        }
    }

    /// Log what reading the session repaired, such as an image whose blob is
    /// gone, and return them.
    pub async fn warn_repairs(&self, session_id: &str) -> Vec<rig_memory::Repair> {
        self.memory.warn_repairs(session_id).await
    }

    pub fn memory(&self) -> &MemoryOf {
        &self.memory
    }

    pub fn memory_mut(&mut self) -> &mut MemoryOf {
        &mut self.memory
    }

    /// Return a reference to the wrapped `CachedMemory` backend.
    ///
    /// Used by code that needs the concrete `CachedMemory` API (store
    /// rewrites, marker writes).
    pub fn inner_memory(&self) -> &CachedMemory {
        &self.memory
    }
}

impl SessionManager for MemoryState {
    type Memory = MemoryOf;
    type InnerMemory = CachedMemory;

    fn memory(&self) -> &Self::Memory {
        &self.memory
    }

    fn memory_mut(&mut self) -> &mut Self::Memory {
        &mut self.memory
    }

    fn inner_memory(&self) -> &Self::InnerMemory {
        &self.memory
    }

    fn rig_store(
        &self,
        session_id: &str,
    ) -> impl std::future::Future<
        Output = Result<Arc<dyn rig_memory::Store>, crate::session::MemoryError>,
    > + Send {
        self.memory.rig_store(session_id)
    }
}

#[cfg(test)]
#[path = "../../../test/conversation/state/memory.rs"]
mod memory_test;
