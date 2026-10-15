//! SQLite session-store integration tests.
//!
//! The suite is split into topical sub-files. Shared imports and the helper
//! re-exports live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "sqlite_store/append.rs"]
mod append;

#[path = "sqlite_store/crud.rs"]
mod crud;

#[path = "sqlite_store/edge_cases.rs"]
mod edge_cases;

#[path = "sqlite_store/list_delete.rs"]
mod list_delete;

#[path = "sqlite_store/persistence.rs"]
mod persistence;

#[path = "sqlite_store/replace.rs"]
mod replace;

#[path = "sqlite_store/title.rs"]
mod title;

// endregion: --- Modules

use super::SqliteSessionStore;
use crate::session::store::{CompactionMarker, SessionStore, StoreEntry};
use crate::types::{CallId, Message, Text, ToolName, ToolResult, ToolResultContent, UserContent};
use chrono::Utc;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;
