use super::store::{
    CompactionMarker, ConversationStore, JsonlConversationStore, StoreEntry, extract_llm_context,
};
use crate::types::Message;
use rig::memory::{ConversationMemory, MemoryError};
use rig::wasm_compat::WasmBoxedFuture;
use std::collections::HashMap;
use std::io;
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
/// to `append_marker`. For completed turns where rig calls `append()` internally, `append()`
/// reads the last known token count from the store and preserves it — so a failed turn that
/// never receives a `CompletionCall` event does not clobber a previously correct token count
/// with null.
#[derive(Clone, Debug)]
pub struct JournalConversationMemory {
    /// In-memory message cache keyed by conversation_id.
    cache: Arc<Mutex<HashMap<String, Vec<Message>>>>,
    /// The JSONL backing store.
    store: Arc<JsonlConversationStore>,
}

impl JournalConversationMemory {
    /// Create a new `JournalConversationMemory` backed by JSONL files in `cache_dir`.
    pub fn new(cache_dir: PathBuf) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            store: Arc::new(JsonlConversationStore::new(cache_dir)),
        }
    }

    /// Write a compaction marker to JSONL only — does not update the in-memory cache.
    ///
    /// Called by the compaction process after computing a summary.
    pub fn append_marker(
        &self,
        conversation_id: &str,
        marker: &CompactionMarker,
        last_total_tokens: Option<u64>,
    ) -> Result<(), io::Error> {
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
    ) -> Result<(), io::Error> {
        self.store
            .append(conversation_id, messages, last_total_tokens)
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
    ) -> Result<(Vec<StoreEntry>, Option<u64>), io::Error> {
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
        self.cache.lock().expect("cache lock poisoned").clear();
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
                    log::trace!("JournalMemory.load: session={conversation_id} cache_hit=true");
                    let (repaired, issues) =
                        crate::session::repair::repair_messages(messages.clone());
                    for issue in &issues {
                        log::warn!("conversation repair (cache hit): {}", issue);
                    }
                    return Ok(repaired);
                }
            }

            // Cache miss — load from JSONL
            log::debug!("JournalMemory.load: session={conversation_id} cache_hit=false");
            let (entries, _last_total_tokens) = self
                .store
                .load_all(conversation_id)
                .map_err(|e| MemoryError::Backend(e.to_string().into()))?;

            // Extract LLM context (handles compaction markers)
            let messages = extract_llm_context(&entries);
            log::debug!(
                "JournalMemory.load: session={conversation_id} messages={}",
                messages.len()
            );

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
            log::trace!(
                "JournalMemory.append: session={conversation_id} count={}",
                messages.len()
            );
            // Append to in-memory cache
            {
                let mut cache = self.lock_cache()?;
                cache
                    .entry(conversation_id.to_string())
                    .or_default()
                    .extend(messages.iter().cloned());
            }

            // Preserve the last known token count: if the store already has a
            // non-null value from a previous turn, use it rather than writing null.
            // This prevents a failed turn (which never receives a CompletionCall
            // event) from clobbering a previously correct token count in JSONL.
            let preserved_tokens = self
                .store
                .load_all(conversation_id)
                .ok()
                .and_then(|(_, tokens)| tokens);

            self.store
                .append(conversation_id, &messages, preserved_tokens)
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
