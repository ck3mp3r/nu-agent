use crate::conversation::managers::SessionManager;
use crate::session::{CachedMemory, RigStoreConfig};
use std::sync::Arc;

/// The full memory type held by `MemoryState` and shared with the turn
/// executor. `CachedMemory` is cheaply cloneable (its caches are `Arc`-backed),
/// so the `Arc` is what gets handed to each turn's agent.
pub type MemoryOf = Arc<CachedMemory>;

pub struct MemoryState {
    memory: MemoryOf,
    last_total_tokens: Option<u64>,
}

impl MemoryState {
    pub fn new(rig: RigStoreConfig) -> Self {
        Self {
            memory: Arc::new(CachedMemory::new(rig)),
            last_total_tokens: None,
        }
    }

    /// Log what reading the session repaired, such as an image whose blob is
    /// gone, and return them.
    pub fn warn_repairs(&self, session_id: &str) -> Vec<rig_memory::Repair> {
        self.memory.warn_repairs(session_id)
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
    /// rewrites, marker writes, raw entry loads).
    pub fn inner_memory(&self) -> &CachedMemory {
        &self.memory
    }

    pub fn last_total_tokens(&self) -> Option<u64> {
        self.last_total_tokens
    }

    pub fn last_total_tokens_mut(&mut self) -> &mut Option<u64> {
        &mut self.last_total_tokens
    }

    pub fn clear(&mut self) {
        self.memory.clear_all();
        self.last_total_tokens = None;
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
    ) -> Result<Arc<dyn rig_memory::Store>, crate::session::MemoryError> {
        self.memory.rig_store(session_id)
    }

    fn clear(&mut self) {
        self.memory.clear_all();
        self.last_total_tokens = None;
    }

    fn last_total_tokens(&self) -> Option<u64> {
        self.last_total_tokens
    }

    fn last_total_tokens_mut(&mut self) -> &mut Option<u64> {
        &mut self.last_total_tokens
    }
}

#[cfg(test)]
#[path = "../../../test/conversation/state/memory.rs"]
mod memory_test;
