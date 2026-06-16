use crate::session::{ConversationStore, JsonlConversationStore, StoreEntry, extract_llm_context};
use crate::types::InMemoryConversationMemory;
use nu_protocol::LabeledError;
use rig::memory::ConversationMemory;

pub struct MemoryState {
    memory: InMemoryConversationMemory,
    conversation_store: JsonlConversationStore,
    memory_hydrated: bool,
    last_total_tokens: Option<u64>,
}

impl MemoryState {
    pub fn new(cache_dir: std::path::PathBuf) -> Self {
        Self {
            memory: InMemoryConversationMemory::new(),
            conversation_store: JsonlConversationStore::new(cache_dir),
            memory_hydrated: false,
            last_total_tokens: None,
        }
    }
    pub fn memory(&self) -> &InMemoryConversationMemory {
        &self.memory
    }
    pub fn memory_mut(&mut self) -> &mut InMemoryConversationMemory {
        &mut self.memory
    }
    pub fn conversation_store(&self) -> &JsonlConversationStore {
        &self.conversation_store
    }
    pub fn last_total_tokens(&self) -> Option<u64> {
        self.last_total_tokens
    }
    pub fn last_total_tokens_mut(&mut self) -> &mut Option<u64> {
        &mut self.last_total_tokens
    }
    pub fn clear(&mut self) {
        self.memory = InMemoryConversationMemory::new();
        self.memory_hydrated = false;
    }
    #[cfg(test)]
    pub fn is_hydrated(&self) -> bool {
        self.memory_hydrated
    }
    pub fn ensure_memory_hydrated(
        &mut self,
        final_session_id: Option<&str>,
        runtime: &tokio::runtime::Runtime,
        compaction_count: &mut usize,
    ) -> Result<(), LabeledError> {
        if self.memory_hydrated {
            return Ok(());
        }
        if let Some(session_id) = final_session_id {
            let (entries, last_total_tokens) = self
                .conversation_store
                .load_all(session_id)
                .map_err(|e| LabeledError::new(format!("Failed to load session entries: {}", e)))?;
            let llm_context = extract_llm_context(&entries);
            if !llm_context.is_empty() {
                runtime
                    .block_on(self.memory.append(session_id, llm_context.clone()))
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to append messages to memory: {}", e))
                    })?;
            }
            self.last_total_tokens = last_total_tokens;
            let marker_count = entries
                .iter()
                .filter(|e| matches!(e, StoreEntry::Marker(_)))
                .count();
            *compaction_count = marker_count;
        }
        self.memory_hydrated = true;
        Ok(())
    }
}
