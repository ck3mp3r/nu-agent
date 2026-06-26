use super::store::{
    CompactionMarker, ConversationStore, JsonlConversationStore, StoreEntry, extract_llm_context,
};
use crate::types::Message;
use rig::memory::{ConversationMemory, MemoryError};
use rig::wasm_compat::WasmBoxedFuture;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A `ConversationMemory` implementation backed by `JsonlConversationStore`.
///
/// Maintains an in-memory cache of messages per conversation to avoid redundant
/// JSONL reads. The cache is populated on first `load` and invalidated by `clear`.
///
/// This replaces the dual-store pattern (`InMemoryConversationMemory` + `JsonlConversationStore`)
/// with a single unified type that rig can use directly.
///
/// ## Token tracking
///
/// Token counts are NOT tracked inside `JournalConversationMemory`. The `ConversationMemory`
/// trait's `append()` signature is fixed by rig and does not carry token information.
/// Token counts are managed by `MemoryState` in the executor layer and written to JSONL
/// explicitly via `append_messages_to_store_only` (cancelled turns) or passed as a parameter
/// to `append_marker`. For completed turns where rig calls `append()` internally, the JSONL
/// entry will have `last_total_tokens: null` — that is acceptable and expected.
#[derive(Clone, Debug)]
pub struct JournalConversationMemory {
    /// In-memory message cache keyed by conversation_id.
    cache: Arc<Mutex<HashMap<String, Vec<Message>>>>,
    /// The JSONL backing store.
    store: Arc<JsonlConversationStore>,
    /// Number of compaction markers seen during the last load.
    compaction_count: Arc<Mutex<usize>>,
}

impl JournalConversationMemory {
    /// Create a new `JournalConversationMemory` backed by JSONL files in `cache_dir`.
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            store: Arc::new(JsonlConversationStore::new(cache_dir)),
            compaction_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Return the number of compaction markers seen in the last `load`.
    pub fn compaction_count(&self) -> usize {
        *self
            .compaction_count
            .lock()
            .expect("compaction_count lock poisoned")
    }

    /// Write a compaction marker to JSONL only — does not update the in-memory cache.
    ///
    /// Called by the compaction process after computing a summary.
    pub fn append_marker(
        &self,
        conversation_id: &str,
        marker: &CompactionMarker,
        last_total_tokens: Option<u64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.store
            .append_marker(conversation_id, marker, last_total_tokens)
    }

    /// Write messages to JSONL only — does not update the in-memory cache.
    ///
    /// Called by the compaction process to re-append kept messages after a marker.
    pub fn append_messages_to_store_only(
        &self,
        conversation_id: &str,
        messages: &[Message],
        last_total_tokens: Option<u64>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.store.append(conversation_id, messages, last_total_tokens)
    }

    /// Replace the in-memory cache for `conversation_id` without touching JSONL.
    ///
    /// Called by the compaction process to update the active context after compaction.
    pub fn reset_context(&self, conversation_id: &str, messages: Vec<Message>) {
        let mut cache = self.cache.lock().expect("cache lock poisoned");
        cache.insert(conversation_id.to_string(), messages);
    }

    /// Load all raw store entries (messages + markers) from JSONL.
    ///
    /// For session resolver / transcript hydration — bypasses the in-memory cache.
    pub fn load_all(
        &self,
        conversation_id: &str,
    ) -> Result<(Vec<StoreEntry>, Option<u64>), Box<dyn std::error::Error>> {
        self.store.load_all(conversation_id)
    }

    /// Return a reference to the backing JSONL store.
    ///
    /// Used by `CompactionExecutor` and session resolver which need direct store access.
    pub fn store(&self) -> &JsonlConversationStore {
        &self.store
    }

    /// Clear all cached conversations from the in-memory cache.
    ///
    /// Used when no conversation_id is available (e.g., `clear_session` in the runtime).
    /// Does not touch JSONL — the store is append-only.
    pub fn clear_all(&self) {
        self.cache
            .lock()
            .expect("cache lock poisoned")
            .clear();
    }

    fn lock_cache(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, HashMap<String, Vec<Message>>>, MemoryError> {
        self.cache
            .lock()
            .map_err(|e| MemoryError::Internal(e.to_string()))
    }
}

impl ConversationMemory for JournalConversationMemory {
    fn load<'a>(
        &'a self,
        conversation_id: &'a str,
    ) -> WasmBoxedFuture<'a, Result<Vec<Message>, MemoryError>> {
        Box::pin(async move {
            // Check cache first
            {
                let cache = self.lock_cache()?;
                if let Some(messages) = cache.get(conversation_id) {
                    return Ok(messages.clone());
                }
            }

            // Cache miss — load from JSONL
            let (entries, _last_total_tokens) = self
                .store
                .load_all(conversation_id)
                .map_err(|e| MemoryError::Backend(e.to_string().into()))?;

            // Count compaction markers
            let marker_count = entries
                .iter()
                .filter(|e| matches!(e, StoreEntry::Marker(_)))
                .count();

            {
                let mut count = self
                    .compaction_count
                    .lock()
                    .map_err(|e| MemoryError::Internal(e.to_string()))?;
                *count = marker_count;
            }

            // Extract LLM context (handles compaction markers)
            let messages = extract_llm_context(&entries);

            // Populate cache
            {
                let mut cache = self.lock_cache()?;
                cache.insert(conversation_id.to_string(), messages.clone());
            }

            Ok(messages)
        })
    }

    fn append<'a>(
        &'a self,
        conversation_id: &'a str,
        messages: Vec<Message>,
    ) -> WasmBoxedFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            // Append to in-memory cache
            {
                let mut cache = self.lock_cache()?;
                cache
                    .entry(conversation_id.to_string())
                    .or_default()
                    .extend(messages.iter().cloned());
            }

            // Write to JSONL store. Token count is not available here — the
            // ConversationMemory trait does not carry it. Tokens are tracked
            // externally by MemoryState and written via append_messages_to_store_only
            // or append_marker for paths that need them.
            self.store
                .append(conversation_id, &messages, None)
                .map_err(|e| MemoryError::Backend(e.to_string().into()))?;

            Ok(())
        })
    }

    fn clear<'a>(
        &'a self,
        conversation_id: &'a str,
    ) -> WasmBoxedFuture<'a, Result<(), MemoryError>> {
        Box::pin(async move {
            let mut cache = self.lock_cache()?;
            cache.remove(conversation_id);
            Ok(())
        })
    }
}
