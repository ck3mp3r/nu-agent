use super::super::managers::SessionManager;
use crate::session::JournalConversationMemory;

pub struct MemoryState {
    memory: JournalConversationMemory,
    last_total_tokens: Option<u64>,
}

impl MemoryState {
    pub fn new(cache_dir: std::path::PathBuf) -> Self {
        Self {
            memory: JournalConversationMemory::new(cache_dir),
            last_total_tokens: None,
        }
    }

    pub fn memory(&self) -> &JournalConversationMemory {
        &self.memory
    }

    pub fn memory_mut(&mut self) -> &mut JournalConversationMemory {
        &mut self.memory
    }

    /// Return the inner JSONL store from the journal memory.
    ///
    /// Used by `CompactionExecutor` and session resolver which need direct store access.
    pub fn conversation_store(&self) -> &crate::session::JsonlConversationStore {
        self.memory.store()
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
    fn memory(&self) -> &JournalConversationMemory {
        &self.memory
    }

    fn memory_mut(&mut self) -> &mut JournalConversationMemory {
        &mut self.memory
    }

    fn clear(&mut self) {
        self.clear()
    }

    fn last_total_tokens(&self) -> Option<u64> {
        self.last_total_tokens
    }

    fn last_total_tokens_mut(&mut self) -> &mut Option<u64> {
        &mut self.last_total_tokens
    }
}
