//! The storage layer of nu-agent: one engine thread per database, one
//! connection pool, and one transport primitive every SQL statement crosses.
//!
//! The engine owns the `SqlitePool` and the tokio runtime its work runs on;
//! a caller submits a [`Work`] unit and receives the value the unit answers.
//! [`Storage::call`] is the async face, [`Storage::call_blocking`] the sync
//! face. `PRAGMA user_version` carries the schema version: every open migrates
//! a database from its stored version to this build's target version, and
//! refuses one that is newer.

// region:    --- Modules

mod engine;
mod error;
mod migrations;
mod rig_adapter;
mod schema;
mod services;
mod storage;

pub use engine::Work;
pub use error::{Result, StorageError};
pub use rig_adapter::RigAdapter;
pub use services::{
    CallFact, Entries, EntryTurn, MessageKey, Settle, SettleRecord, ToolCallStatus, ToolCalls,
    TurnFacts, Turns,
};
pub use storage::Storage;

// endregion: --- Modules

// region:    --- Tests

#[cfg(test)]
#[path = "../test/lib.rs"]
mod lib_test;

#[cfg(test)]
#[path = "../test/services/test.rs"]
mod services_test;

// endregion: --- Tests
