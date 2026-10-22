//! rig-memory SQLite store integration tests.
//!
//! The suite is split into topical sub-files. Shared imports and helpers live
//! here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "rig_sqlite_store/blobs.rs"]
mod blobs;

#[path = "rig_sqlite_store/files.rs"]
mod files;

#[path = "rig_sqlite_store/legacy.rs"]
mod legacy;

#[path = "rig_sqlite_store/logs.rs"]
mod logs;

#[path = "rig_sqlite_store/seam.rs"]
mod seam;

// endregion: --- Modules

use super::{RigSqliteStore, RigStoreError, StoreType, create_rig_store_in};
use crate::types::Message;
use rig::completion::message::{DocumentSourceKind, ImageMediaType, UserContent};
use rig_memory::{Files, LogWriteError, Store, load_images, store_images};
use sha2::{Digest, Sha256};
use tempfile::TempDir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A store over an in-memory database for `session_id`.
fn memory_store(session_id: &str) -> Result<RigSqliteStore> {
    Ok(RigSqliteStore::in_memory(session_id)?)
}

/// A user message that carries one PNG image of `bytes`.
fn image_message(bytes: &[u8]) -> Message {
    Message::User {
        content: vec![
            UserContent::text("look"),
            UserContent::image_raw(bytes.to_vec(), Some(ImageMediaType::PNG), None),
        ],
    }
}

/// `bytes` as lowercase hexadecimal.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
