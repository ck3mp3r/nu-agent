use nu_protocol::LabeledError;
use rig::memory::ConversationMemory;

use crate::session::{ConversationStore, JsonlConversationStore, StoreEntry, extract_llm_context};
use crate::types::InMemoryConversationMemory;

pub(crate) struct MemoryState {
    pub(crate) memory: InMemoryConversationMemory,
    pub(crate) conversation_store: JsonlConversationStore,
    pub(crate) memory_hydrated: bool,
    pub(crate) last_total_tokens: Option<u64>,
}

impl MemoryState {
    /// Idempotent memory hydration: loads stored messages into in-memory
    /// conversation memory exactly once per runtime lifetime (or until
    /// `clear_session` resets the guard).
    pub(crate) fn ensure_memory_hydrated(
        &mut self,
        final_session_id: Option<&str>,
        runtime: &tokio::runtime::Runtime,
        compaction_count: &mut usize,
    ) -> Result<(), LabeledError> {
        if self.memory_hydrated {
            return Ok(());
        }
        if let Some(session_id) = final_session_id {
            // Load ALL entries (messages + markers)
            let (entries, last_total_tokens) = self
                .conversation_store
                .load_all(session_id)
                .map_err(|e| LabeledError::new(format!("Failed to load session entries: {}", e)))?;

            // Extract only LLM-relevant messages (from latest marker onward)
            let llm_context = extract_llm_context(&entries);

            if !llm_context.is_empty() {
                runtime
                    .block_on(self.memory.append(session_id, llm_context.clone()))
                    .map_err(|e| {
                        LabeledError::new(format!("Failed to append messages to memory: {}", e))
                    })?;
            }
            self.last_total_tokens = last_total_tokens;

            // Derive compaction_count from markers
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
