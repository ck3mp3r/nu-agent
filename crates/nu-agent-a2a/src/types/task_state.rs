use std::fmt;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// TaskState (A2A spec §9.6)
// ---------------------------------------------------------------------------

/// The state of an A2A task.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TaskState {
    #[default]
    Unspecified,
    Submitted,
    Working,
    InputRequired,
    Completed,
    Failed,
    Canceled,
    Rejected,
    AuthRequired,
}

impl fmt::Display for TaskState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            TaskState::Unspecified => "TASK_STATE_UNSPECIFIED",
            TaskState::Submitted => "TASK_STATE_SUBMITTED",
            TaskState::Working => "TASK_STATE_WORKING",
            TaskState::InputRequired => "TASK_STATE_INPUT_REQUIRED",
            TaskState::Completed => "TASK_STATE_COMPLETED",
            TaskState::Failed => "TASK_STATE_FAILED",
            TaskState::Canceled => "TASK_STATE_CANCELED",
            TaskState::Rejected => "TASK_STATE_REJECTED",
            TaskState::AuthRequired => "TASK_STATE_AUTH_REQUIRED",
        };
        f.write_str(s)
    }
}

impl TryFrom<&str> for TaskState {
    type Error = String;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        match s {
            // SCREAMING_SNAKE_CASE (spec format)
            "TASK_STATE_UNSPECIFIED" => Ok(TaskState::Unspecified),
            "TASK_STATE_SUBMITTED" => Ok(TaskState::Submitted),
            "TASK_STATE_WORKING" => Ok(TaskState::Working),
            "TASK_STATE_INPUT_REQUIRED" => Ok(TaskState::InputRequired),
            "TASK_STATE_COMPLETED" => Ok(TaskState::Completed),
            "TASK_STATE_FAILED" => Ok(TaskState::Failed),
            "TASK_STATE_CANCELED" => Ok(TaskState::Canceled),
            "TASK_STATE_REJECTED" => Ok(TaskState::Rejected),
            "TASK_STATE_AUTH_REQUIRED" => Ok(TaskState::AuthRequired),
            // Legacy lowercase strings (backward compat)
            "submitted" => Ok(TaskState::Submitted),
            "working" => Ok(TaskState::Working),
            "inputRequired" => Ok(TaskState::InputRequired),
            "completed" => Ok(TaskState::Completed),
            "failed" => Ok(TaskState::Failed),
            "canceled" => Ok(TaskState::Canceled),
            "rejected" => Ok(TaskState::Rejected),
            _ => Err(format!("unknown task state: {s}")),
        }
    }
}
