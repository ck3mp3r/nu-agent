use super::journal::CachedMemory;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A `CachedMemory` whose rig-memory store is in memory.
fn test_memory() -> CachedMemory {
    CachedMemory::new(crate::session::RigStoreConfig::in_memory())
}

// region:    --- Modules

#[path = "journal/bound.rs"]
mod bound;

// endregion: --- Modules
