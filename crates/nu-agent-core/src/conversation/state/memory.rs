use super::super::managers::SessionManager;
use crate::session::{CachedMemory, FsSessionStore, SessionStore};
use std::path::PathBuf;
use std::sync::Arc;

pub struct MemoryState<S: SessionStore + Clone + Send + Sync> {
    memory: CachedMemory<S>,
    last_total_tokens: Option<u64>,
}

impl<S: SessionStore + Clone + Send + Sync> MemoryState<S> {
    pub fn new(store: Arc<S>) -> Self {
        Self {
            memory: CachedMemory::new(store),
            last_total_tokens: None,
        }
    }

    pub fn memory(&self) -> &CachedMemory<S> {
        &self.memory
    }

    pub fn memory_mut(&mut self) -> &mut CachedMemory<S> {
        &mut self.memory
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

impl MemoryState<FsSessionStore> {
    /// Convenience constructor that creates an `FsSessionStore` from a path.
    pub fn with_path(base_path: PathBuf) -> Self {
        Self::new(Arc::new(FsSessionStore::new(base_path)))
    }
}

impl<S: SessionStore + Clone + Send + Sync> SessionManager for MemoryState<S> {
    type Memory = CachedMemory<S>;

    fn memory(&self) -> &Self::Memory {
        &self.memory
    }

    fn memory_mut(&mut self) -> &mut Self::Memory {
        &mut self.memory
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
