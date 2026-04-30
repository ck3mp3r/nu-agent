use super::super::providers::cached::CachedProvider;
use super::super::providers::key::ProviderKey;
use nu_protocol::LabeledError;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

type ProviderCell = Arc<Mutex<Option<Arc<CachedProvider>>>>;

#[derive(Clone, Default)]
pub struct ProviderCache {
    entries: Arc<RwLock<HashMap<ProviderKey, ProviderCell>>>,
    hit_count: Arc<AtomicU64>,
    miss_count: Arc<AtomicU64>,
    create_count: Arc<AtomicU64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderCacheStats {
    pub hits: u64,
    pub misses: u64,
    pub creates: u64,
}

impl ProviderCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get_or_create<F>(
        &self,
        key: ProviderKey,
        builder: F,
    ) -> Result<Arc<CachedProvider>, LabeledError>
    where
        F: FnOnce() -> Result<CachedProvider, LabeledError>,
    {
        let cell = {
            if let Ok(read_guard) = self.entries.read() {
                if let Some(existing) = read_guard.get(&key) {
                    existing.clone()
                } else {
                    drop(read_guard);
                    let mut write_guard = self
                        .entries
                        .write()
                        .map_err(|_| LabeledError::new("Provider cache write lock poisoned"))?;
                    write_guard
                        .entry(key)
                        .or_insert_with(|| Arc::new(Mutex::new(None)))
                        .clone()
                }
            } else {
                return Err(LabeledError::new("Provider cache read lock poisoned"));
            }
        };

        let mut cell_guard = cell
            .lock()
            .map_err(|_| LabeledError::new("Provider cache cell lock poisoned"))?;

        if let Some(existing) = &*cell_guard {
            self.hit_count.fetch_add(1, Ordering::Relaxed);
            return Ok(existing.clone());
        }

        self.miss_count.fetch_add(1, Ordering::Relaxed);
        let created = Arc::new(builder()?);
        *cell_guard = Some(created.clone());
        self.create_count.fetch_add(1, Ordering::Relaxed);
        Ok(created)
    }

    pub fn stats(&self) -> ProviderCacheStats {
        ProviderCacheStats {
            hits: self.hit_count.load(Ordering::Relaxed),
            misses: self.miss_count.load(Ordering::Relaxed),
            creates: self.create_count.load(Ordering::Relaxed),
        }
    }
}

#[cfg(test)]
#[path = "test.rs"]
mod test;
