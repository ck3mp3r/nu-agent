use super::store::{CompactionMarker, SessionStore, StoreEntry};
use crate::types::Message;
use rig::memory::{ConversationMemory, MemoryError};
use rig::wasm_compat::WasmBoxedFuture;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// A `ConversationMemory` implementation backed by any `S: SessionStore`.
///
/// Maintains an in-memory cache of messages per conversation to avoid redundant
/// store reads. The cache is populated on first `load` and invalidated by `clear`.
///
/// This replaces the dual-store pattern (`InMemoryConversationMemory` + concrete store)
/// with a single unified type that rig can use directly, generic over the backing store.
///
/// ## Token tracking
///
/// Token counts are NOT tracked inside `CachedMemory`. The `ConversationMemory`
/// trait's `append()` signature is fixed by rig and does not carry token information.
/// Token counts are managed by `MemoryState` in the executor layer.
pub struct CachedMemory<S: SessionStore + Clone + Send + Sync> {
    /// In-memory message cache keyed by conversation_id.
    cache: Arc<Mutex<HashMap<String, Vec<Message>>>>,
    /// Track which conversation_ids have been persisted (for create vs append).
    persisted: Arc<Mutex<HashSet<String>>>,
    /// The backing session store.
    store: Arc<S>,
}

impl<S: SessionStore + Clone + Send + Sync> Clone for CachedMemory<S> {
    fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
            persisted: Arc::clone(&self.persisted),
            store: Arc::clone(&self.store),
        }
    }
}

impl<S: SessionStore + Clone + Send + Sync> std::fmt::Debug for CachedMemory<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedMemory").finish_non_exhaustive()
    }
}

impl<S: SessionStore + Clone + Send + Sync> CachedMemory<S> {
    /// Create a new `CachedMemory` backed by the given store.
    pub fn new(store: Arc<S>) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            persisted: Arc::new(Mutex::new(HashSet::new())),
            store,
        }
    }

    /// Write a compaction marker to the store — does not update the in-memory cache.
    ///
    /// Called by the compaction process after computing a summary.
    pub async fn append_marker(
        &self,
        conversation_id: &str,
        marker: &CompactionMarker,
    ) -> Result<(), S::Error> {
        self.store
            .append(conversation_id, &[StoreEntry::Marker(marker.clone())])
            .await
    }

    /// Append messages to the store only — does not update the in-memory cache.
    ///
    /// Called by the compaction process to re-append kept messages after a marker.
    pub async fn append_messages_to_store_only(
        &self,
        conversation_id: &str,
        messages: &[Message],
    ) -> Result<(), S::Error> {
        let entries: Vec<StoreEntry> = messages.iter().cloned().map(StoreEntry::Message).collect();
        self.store.append(conversation_id, &entries).await
    }

    /// Replace the in-memory cache for `conversation_id` without touching the store.
    ///
    /// Called by the compaction process to update the active context after compaction.
    pub fn reset_context(&self, conversation_id: &str, messages: Vec<Message>) {
        let mut cache = self.cache.lock().expect("cache lock poisoned");
        cache.insert(conversation_id.to_string(), messages);
    }

    /// Clone the in-memory cache entry for `conversation_id`, if present.
    ///
    /// Read-only peek used by the trimming wrapper to distinguish a summary
    /// that came from the cache (seeded via `seed_from_store`) from one that
    /// the `CompactingMemory` just spliced from its in-process state after a
    /// real compaction. `None` when the conversation is not yet cached.
    pub fn cached_messages(&self, conversation_id: &str) -> Option<Vec<Message>> {
        self.cache
            .lock()
            .map(|c| c.get(conversation_id).cloned())
            .ok()
            .flatten()
    }

    /// Load all raw store entries (messages + markers).
    ///
    /// For session resolver / transcript hydration — bypasses the in-memory cache.
    pub async fn load_all(&self, conversation_id: &str) -> Result<Vec<StoreEntry>, S::Error> {
        match self.store.load(conversation_id).await? {
            Some((_metadata, entries)) => Ok(entries),
            None => Ok(Vec::new()),
        }
    }

    /// Replace all entries for a conversation in the store (compaction rewrite).
    pub async fn replace_entries(
        &self,
        conversation_id: &str,
        entries: &[StoreEntry],
    ) -> Result<(), S::Error> {
        self.store.replace_entries(conversation_id, entries).await
    }

    /// Clear all cached conversations from the in-memory cache.
    ///
    /// Used when no conversation_id is available (e.g., `clear_session` in the runtime).
    /// Does not touch the store — it is append-only.
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

impl<S: SessionStore + Clone + Send + Sync> ConversationMemory for CachedMemory<S> {
    fn load<'a>(
        &'a self,
        conversation_id: &'a str,
    ) -> WasmBoxedFuture<'a, Result<Vec<Message>, MemoryError>> {
        Box::pin(async move {
            // Check cache first
            {
                let cache = self.lock_cache()?;
                if let Some(messages) = cache.get(conversation_id) {
                    log::trace!("CachedMemory.load: session={conversation_id} cache_hit=true");
                    let (repaired, issues) =
                        crate::session::repair::repair_messages(messages.clone());
                    for issue in &issues {
                        log::warn!("conversation repair (cache hit): {}", issue);
                    }
                    return Ok(repaired);
                }
            }

            // Cache miss — load from store
            log::debug!("CachedMemory.load: session={conversation_id} cache_hit=false");
            let entries = match self.store.load(conversation_id).await {
                Ok(Some((_metadata, entries))) => entries,
                Ok(None) => {
                    log::debug!("CachedMemory.load: session={conversation_id} no entries found");
                    // Populate cache with empty vec so subsequent loads are cache hits,
                    // even if the store is later modified (e.g., by append_marker).
                    let mut cache = self.lock_cache()?;
                    cache.insert(conversation_id.to_string(), Vec::new());
                    return Ok(Vec::new());
                }
                Err(e) => {
                    return Err(MemoryError::Backend(e.to_string().into()));
                }
            };

            // Extract raw messages from store entries (markers are left in place;
            // the CompactingMemory wrapper applies its policy on load).
            let messages = entries
                .iter()
                .filter_map(|e| match e {
                    StoreEntry::Message(m) => Some(m.clone()),
                    _ => None,
                })
                .collect::<Vec<Message>>();
            let (messages, issues) = crate::session::repair::repair_messages(messages);
            for issue in &issues {
                log::warn!("conversation repair: {}", issue);
            }
            log::debug!(
                "CachedMemory.load: session={conversation_id} messages={}",
                messages.len()
            );

            // Populate cache and mark as persisted so subsequent appends
            // call store.append() instead of store.create() which would truncate.
            {
                let mut cache = self.lock_cache()?;
                cache.insert(conversation_id.to_string(), messages.clone());
            }
            {
                let mut persisted = self.persisted.lock().expect("persisted lock poisoned");
                persisted.insert(conversation_id.to_string());
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
                "CachedMemory.append: session={conversation_id} count={}",
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

            // First-write detection: on first write, call store.create();
            // subsequent writes call store.append().
            let is_first_write = {
                let mut persisted = self.persisted.lock().expect("persisted lock poisoned");
                persisted.insert(conversation_id.to_string())
            };

            if is_first_write {
                self.store
                    .create(conversation_id, &messages)
                    .await
                    .map_err(|e| MemoryError::Backend(e.to_string().into()))?;
            } else {
                let entries: Vec<StoreEntry> =
                    messages.into_iter().map(StoreEntry::Message).collect();
                self.store
                    .append(conversation_id, &entries)
                    .await
                    .map_err(|e| MemoryError::Backend(e.to_string().into()))?;
            }

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
