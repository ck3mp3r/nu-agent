use crate::conversation::managers::SessionManager;
use crate::session::{CachedMemory, SessionStore};
use std::sync::Arc;

/// The full memory type held by `MemoryState` and shared with the turn
/// executor. `CachedMemory` is cheaply cloneable (its caches are `Arc`-backed),
/// so the `Arc` is what gets handed to each turn's agent.
pub type MemoryOf<S> = Arc<CachedMemory<S>>;

pub struct MemoryState<S: SessionStore + Clone + Send + Sync> {
    memory: MemoryOf<S>,
    last_total_tokens: Option<u64>,
}

impl<S: SessionStore + Clone + Send + Sync> MemoryState<S> {
    pub fn new(store: Arc<S>) -> Self {
        Self {
            memory: Arc::new(CachedMemory::new(store)),
            last_total_tokens: None,
        }
    }

    pub fn memory(&self) -> &MemoryOf<S> {
        &self.memory
    }

    pub fn memory_mut(&mut self) -> &mut MemoryOf<S> {
        &mut self.memory
    }

    /// Return a reference to the wrapped `CachedMemory` backend.
    ///
    /// Used by code that needs the concrete `CachedMemory` API (store
    /// rewrites, marker writes, raw entry loads).
    pub fn inner_memory(&self) -> &CachedMemory<S> {
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

impl<S: SessionStore + Clone + Send + Sync> SessionManager for MemoryState<S> {
    type Memory = MemoryOf<S>;
    type InnerMemory = CachedMemory<S>;

    fn memory(&self) -> &Self::Memory {
        &self.memory
    }

    fn memory_mut(&mut self) -> &mut Self::Memory {
        &mut self.memory
    }

    fn inner_memory(&self) -> &Self::InnerMemory {
        &self.memory
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
#[path = "memory_test.rs"]
mod memory_test;
