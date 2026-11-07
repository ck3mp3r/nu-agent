use chrono::{DateTime, Utc};

/// Information about a session, extracted from metadata without loading all messages.
/// Used for listing sessions efficiently.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionInfo {
    /// Session identifier
    pub id: String,
    /// Number of messages in the session (excluding metadata line)
    pub message_count: usize,
    /// When the session last changed, when the store recorded a real instant.
    /// Unknown — no stamp, or one the record cannot stand behind — is `None`:
    /// the picker shows a blank cell rather than a faked clock reading.
    pub last_active: Option<DateTime<Utc>>,
    /// Optional title extracted from the first user message
    pub title: Option<String>,
    /// The agent of the session's most recent recorded turn, when one is
    /// recorded and named. A session with no turns, or whose last turn ran
    /// persona-less, carries `None`.
    pub agent: Option<String>,
}
