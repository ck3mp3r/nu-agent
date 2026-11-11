//! The storage layer of nu-agent: one engine thread per database, one
//! connection pool, and one transport primitive every SQL statement crosses.
//!
//! The engine owns the `SqlitePool` and the tokio runtime its work runs on;
//! a caller submits a [`Work`] unit and receives the value the unit answers.
//! [`Storage::call`] is the async face, [`Storage::call_blocking`] the sync
//! face. The `sqlx` Migrator carries the schema: every open runs this build's
//! embedded migrations, the framework records what it applied in
//! `_sqlx_migrations`, and a database it cannot migrate fails the open.

// region:    --- Modules

mod engine;
mod error;
mod rig_adapter;
mod services;
mod storage;
mod transcript;

pub use engine::Work;
pub use error::{Result, StorageError};
pub use rig_adapter::RigAdapter;
pub use services::{
    CallFact, Entries, EntryTurn, MessageKey, Settle, SettleRecord, ToolCallStatus, ToolCalls,
    TurnFacts, Turns,
};
pub use storage::Storage;
pub use transcript::{
    MediaKind, MediaOwner, MediaSource, TranscriptId, TranscriptRecord, TranscriptRow,
    TranscriptStamp,
};

// endregion: --- Modules

// region:    --- Tests

#[cfg(test)]
#[path = "../test/lib.rs"]
mod lib_test;

#[cfg(test)]
#[path = "../test/services/test.rs"]
mod services_test;

// endregion: --- Tests
