use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::message::Message;
use super::serde_helpers::{deserialize_rfc3339_flexible, serialize_rfc3339_z};
use super::task_state::TaskState;

// ---------------------------------------------------------------------------
// TaskStatus (A2A spec §9.3)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskStatus {
    pub state: TaskState,
    #[serde(
        serialize_with = "serialize_rfc3339_z",
        deserialize_with = "deserialize_rfc3339_flexible"
    )]
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
}
