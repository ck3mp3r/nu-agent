use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Represents a session with its ID and metadata.
/// For now, this is a minimal struct that will be expanded in later tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    id: String,
    created_at: DateTime<Utc>,
}

impl Session {
    /// Returns the session ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Creates a new session with the given ID.
    pub fn new(id: String) -> Self {
        Self {
            id,
            created_at: Utc::now(),
        }
    }

    /// Returns the session creation timestamp.
    pub fn created_at(&self) -> &DateTime<Utc> {
        &self.created_at
    }
}
