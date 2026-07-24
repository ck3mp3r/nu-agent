pub mod factory;
pub mod journal;
pub mod prefix;
pub(crate) mod repair;
pub mod resolver;
pub mod sqlite_store;
mod store;

#[cfg(test)]
#[path = "prefix_test.rs"]
mod prefix_test;

#[cfg(test)]
#[path = "store_test.rs"]
mod store_test;

#[cfg(test)]
#[path = "journal_test.rs"]
mod journal_test;

#[cfg(test)]
#[path = "repair_test.rs"]
mod repair_test;

#[cfg(test)]
#[path = "resolver_test.rs"]
mod resolver_test;

pub use factory::{SessionStoreImpl, StoreError, StoreType, create_store};
pub use journal::CachedMemory;
pub use store::{CompactionMarker, FsSessionStore, SessionStore, StoreEntry, extract_llm_context};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::compaction::CompactionParams;

/// Represents a session with its ID and metadata.
/// For now, this is a minimal struct that will be expanded in later tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    id: String,
    created_at: DateTime<Utc>,
    #[serde(default)]
    config: CompactionParams,
}

/// Information about a session, extracted from metadata without loading all messages.
/// Used for listing sessions efficiently.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    /// Session identifier
    pub id: String,
    /// Number of messages in the session (excluding metadata line)
    pub message_count: usize,
    /// Timestamp of last activity (currently same as created_at)
    pub last_active: DateTime<Utc>,
}

impl Session {
    /// Returns the session ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Creates a new session with the given ID.
    fn new(id: String) -> Self {
        Self {
            id,
            created_at: Utc::now(),
            config: CompactionParams::default(),
        }
    }

    /// Sets the session compaction configuration.
    pub fn set_compaction_config(&mut self, config: CompactionParams) {
        self.config = config;
    }

    /// Returns the session compaction configuration.
    pub fn compaction_config(&self) -> &CompactionParams {
        &self.config
    }

    /// Returns the session creation timestamp.
    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.created_at
    }

    /// Create a Session from stored metadata.
    pub(crate) fn from_metadata(metadata: SessionMetadata) -> Self {
        Self {
            id: metadata.session_id,
            created_at: metadata.created_at,
            config: CompactionParams::default(),
        }
    }
}

/// Metadata stored as the first line of a JSONL file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    #[serde(rename = "type")]
    pub metadata_type: String,
    pub session_id: String,
    pub created_at: DateTime<Utc>,
}

#[cfg(test)]
mod tool_session_test;
