use super::factory::RigStoreConfig;
use super::memory_error::MemoryError;
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

/// The rig-memory stores the agent writes the conversation through.
///
/// The agent writes the conversation through rig; this type only reads. A
/// session's store is opened on first use and kept, so repeated reads do not
/// reopen it.
///
/// ## Token tracking
///
/// Token counts are NOT tracked inside `CachedMemory`. Token counts are managed
/// by `MemoryState` in the executor layer.
pub struct CachedMemory {
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
}
