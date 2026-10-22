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

impl Session {
    /// Returns the session ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Creates a new session with the given ID.
    pub(crate) fn new(id: String) -> Self {
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
}
