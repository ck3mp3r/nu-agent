use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::message::Message;
use super::task_state::TaskState;

// ---------------------------------------------------------------------------
// TaskStatus (A2A spec §9.3)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TaskStatus {
    pub state: TaskState,
    pub timestamp: DateTime<Utc>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
}
