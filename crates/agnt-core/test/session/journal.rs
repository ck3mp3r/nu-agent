use super::journal::CachedMemory;
use super::store::{CompactionMarker, FsSessionStore, SessionStore as _, StoreEntry};
use super::store_test::{assert_msg_eq, assert_msgs_eq};
use crate::types::Message;
use chrono::Utc;
use rig::id::ConversationId;
use rig::memory::ConversationMemory;
use std::sync::Arc;
use tempfile::TempDir;

type TestMemory = CachedMemory<FsSessionStore>;
type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Modules

#[path = "journal/append_clear.rs"]
mod append_clear;

#[path = "journal/dedup.rs"]
mod dedup;

#[path = "journal/load.rs"]
mod load;

#[path = "journal/support.rs"]
mod support;

#[path = "journal/verdict.rs"]
mod verdict;

// endregion: --- Modules
