//! The session's store and fact tables, as a turn opens them.
//!
//! A turn opens two things from the session's memory: the rig-memory store it
//! keeps the conversation in, and the fact tables its settle writes. Either open
//! can refuse the database. The turn asks [`TurnMemory`] rather than the
//! concrete backend, so a test substitutes a memory that refuses without a
//! database.

use std::future::Future;
use std::sync::Arc;

use crate::conversation::state::memory::MemoryOf;
use crate::session::MemoryError;
use crate::session::settle::FactStore;

/// The session's memory, as a turn opens it.
pub trait TurnMemory: Sync {
    /// The memory the hook chain writes and compacts through.
    fn memory(&self) -> MemoryOf;

    /// The rig-memory store of `session_id`, made on first use and kept.
    fn rig_store(
        &self,
        session_id: &str,
    ) -> impl Future<Output = Result<Arc<dyn rig_memory::Store>, MemoryError>> + Send;

    /// The fact tables the turn's settle writes.
    fn facts(&self) -> impl Future<Output = Result<FactStore, MemoryError>> + Send;
}

impl TurnMemory for MemoryOf {
    fn memory(&self) -> MemoryOf {
        Arc::clone(self)
    }

    fn rig_store(
        &self,
        session_id: &str,
    ) -> impl Future<Output = Result<Arc<dyn rig_memory::Store>, MemoryError>> + Send {
        self.as_ref().rig_store(session_id)
    }

    fn facts(&self) -> impl Future<Output = Result<FactStore, MemoryError>> + Send {
        self.as_ref().facts()
    }
}

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/conversation/turn/store.rs"]
mod store_test;

// endregion: --- Tests
