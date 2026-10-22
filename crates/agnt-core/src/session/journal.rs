use super::factory::RigStoreConfig;
use super::memory_error::MemoryError;
use crate::types::Message;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// How many sessions' rig-memory stores stay open at once.
///
/// A store is opened on first use and kept, so a long-lived process that
/// touches many sessions would otherwise hold one open store per session for
/// its whole life. The oldest open store is dropped when the bound is reached;
/// a turn that still holds it keeps it alive through its `Arc`, and the next
/// access reopens it.
const OPEN_STORES: usize = 32;

/// A read-through cache over the rig-memory store.
///
/// The agent writes the conversation through rig; this type only reads. The
/// cache is populated on first `load` and invalidated by `clear`.
///
/// ## Token tracking
///
/// Token counts are NOT tracked inside `CachedMemory`. Token counts are managed
/// by `MemoryState` in the executor layer.
pub struct CachedMemory {
    /// In-memory message cache keyed by conversation_id.
    cache: Arc<Mutex<HashMap<String, Vec<Message>>>>,
    /// How the rig-memory store of a session is made.
    rig: RigStoreConfig,
    /// The rig-memory store of each session, made on first use.
    rig_stores: Arc<Mutex<OpenStores>>,
}

/// The open rig-memory stores, oldest first.
#[derive(Default)]
struct OpenStores {
    stores: HashMap<String, Arc<dyn rig_memory::Store>>,
    order: VecDeque<String>,
}

impl Clone for CachedMemory {
    fn clone(&self) -> Self {
        Self {
            cache: Arc::clone(&self.cache),
            rig: self.rig.clone(),
            rig_stores: Arc::clone(&self.rig_stores),
        }
    }
}

impl std::fmt::Debug for CachedMemory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedMemory").finish_non_exhaustive()
    }
}

impl CachedMemory {
    /// Create a new `CachedMemory` backed by the given rig-memory store config.
    pub fn new(rig: RigStoreConfig) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            rig,
            rig_stores: Arc::new(Mutex::new(OpenStores::default())),
        }
    }

    /// The rig-memory store of `session_id`, made on first use and kept.
    ///
    /// At most [`OPEN_STORES`] stores stay open; the oldest is dropped when the
    /// bound is reached.
    ///
    /// # Errors
    ///
    /// When the store cannot be opened.
    pub fn rig_store(&self, session_id: &str) -> Result<Arc<dyn rig_memory::Store>, MemoryError> {
        let mut open = self.rig_stores.lock().expect("rig stores lock poisoned");
        if let Some(store) = open.stores.get(session_id) {
            return Ok(Arc::clone(store));
        }
        let store = self.rig.open(session_id).map_err(MemoryError::backend)?;
        open.stores
            .insert(session_id.to_string(), Arc::clone(&store));
        open.order.push_back(session_id.to_string());
        while open.order.len() > OPEN_STORES {
            if let Some(oldest) = open.order.pop_front() {
                open.stores.remove(&oldest);
            }
        }
        Ok(store)
    }

    /// The ids of the sessions the rig-memory store holds.
    ///
    /// The ids of the stores already opened are included: an in-memory
    /// backend keeps nothing across opens, so its cache is the only record of
    /// the sessions it holds.
    ///
    /// # Errors
    ///
    /// When the store cannot be listed.
    pub fn session_ids(&self) -> Result<Vec<String>, MemoryError> {
        let mut ids = self.rig.session_ids().map_err(MemoryError::backend)?;
        let opened: Vec<String> = self
            .rig_stores
            .lock()
            .expect("rig stores lock poisoned")
            .stores
            .keys()
            .cloned()
            .collect();
        for id in opened {
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        ids.sort();
        Ok(ids)
    }

    /// Whether the rig-memory store holds a log for `session_id`.
    ///
    /// A store already opened counts as holding its session. Otherwise the
    /// store is opened without keeping it, so probing an absent session does
    /// not register it.
    ///
    /// # Errors
    ///
    /// When the store cannot be opened or listed.
    pub fn rig_holds(&self, session_id: &str) -> Result<bool, MemoryError> {
        if self
            .rig_stores
            .lock()
            .expect("rig stores lock poisoned")
            .stores
            .contains_key(session_id)
        {
            return Ok(true);
        }
        let store = self.rig.open(session_id).map_err(MemoryError::backend)?;
        let logs = store.logs().map_err(MemoryError::backend)?;
        Ok(logs.iter().any(|log| log == session_id))
    }

    /// Log what reading the session repaired, such as an image whose blob is
    /// gone, and return them. A store that cannot be read is logged too: the
    /// session still opens, and the next save reports the failure.
    pub fn warn_repairs(&self, session_id: &str) -> Vec<rig_memory::Repair> {
        let store = match self.rig_store(session_id) {
            Ok(store) => store,
            Err(error) => {
                log::warn!("Failed to open the session store: {error}");
                return Vec::new();
            }
        };
        match rig_memory::Session::read(&*store) {
            Ok(session) => {
                for repair in &session.repairs {
                    log::warn!("session repair: {repair}");
                }
                session.repairs
            }
            Err(error) => {
                log::warn!("Failed to read the session: {error}");
                Vec::new()
            }
        }
    }

    /// The messages the rig-memory store holds for `conversation_id`, as rig
    /// reads them: unrepaired, so the count matches the history a run reports.
    fn rig_messages(&self, conversation_id: &str) -> Result<Vec<Message>, MemoryError> {
        let store = self.rig_store(conversation_id)?;
        let conversation =
            rig_memory::load(&*store, conversation_id).map_err(MemoryError::backend)?;
        Ok(conversation
            .messages()
            .iter()
            .map(|message| (**message).clone())
            .collect())
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

impl CachedMemory {
    /// Load the full conversation history for `conversation_id`.
    ///
    /// Returns an empty `Vec` if the conversation has no stored messages. The
    /// messages are read from the rig-memory store, which the agent writes, and
    /// pass through the repair pipeline.
    pub async fn load(&self, conversation_id: &str) -> Result<Vec<Message>, MemoryError> {
        // Check cache first
        {
            let cache = self.lock_cache()?;
            if let Some(messages) = cache.get(conversation_id) {
                log::trace!("CachedMemory.load: session={conversation_id} cache_hit=true");
                let (repaired, issues) = crate::session::repair::repair_messages(messages.clone());
                for issue in &issues {
                    log::warn!("conversation repair (cache hit): {}", issue);
                }
                return Ok(repaired);
            }
        }

        // Cache miss — read the conversation from the rig-memory store, the
        // source of truth for what the agent saved.
        log::debug!("CachedMemory.load: session={conversation_id} cache_hit=false");
        let stored = self.rig_messages(conversation_id)?;
        let (messages, issues) = crate::session::repair::repair_messages(stored);
        for issue in &issues {
            log::warn!("conversation repair: {}", issue);
        }
        log::debug!(
            "CachedMemory.load: session={conversation_id} messages={}",
            messages.len()
        );

        // Populate the cache so later reads are cache hits.
        {
            let mut cache = self.lock_cache()?;
            cache.insert(conversation_id.to_string(), messages.clone());
        }

        Ok(messages)
    }

    /// Remove all cached messages for `conversation_id`.
    ///
    /// Does not touch the store — it is append-only.
    pub async fn clear(&self, conversation_id: &str) -> Result<(), MemoryError> {
        let mut cache = self.lock_cache()?;
        cache.remove(conversation_id);
        Ok(())
    }
}
