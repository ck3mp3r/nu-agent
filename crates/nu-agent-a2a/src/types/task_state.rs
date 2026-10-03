use std::fmt;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// TaskState (A2A spec §9.6)
// ---------------------------------------------------------------------------

/// The state of an A2A task.
#[derive(Clone, Debug, PartialEq, Default, Serialize, Deserialize)]
pub enum TaskState {
    #[default]
    #[serde(rename = "TASK_STATE_UNSPECIFIED")]
    Unspecified,
    #[serde(rename = "TASK_STATE_SUBMITTED")]
    Submitted,
    #[serde(rename = "TASK_STATE_WORKING")]
    Working,
    #[serde(rename = "TASK_STATE_INPUT_REQUIRED")]
    InputRequired,
    #[serde(rename = "TASK_STATE_COMPLETED")]
    Completed,
    #[serde(rename = "TASK_STATE_FAILED")]
    Failed,
    #[serde(rename = "TASK_STATE_CANCELED")]
    Canceled,
    #[serde(rename = "TASK_STATE_REJECTED")]
    Rejected,
    #[serde(rename = "TASK_STATE_AUTH_REQUIRED")]
    AuthRequired,
}

impl TaskState {
    /// Returns `true` if the task is in a terminal state (no further state transitions).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskState::Completed | TaskState::Failed | TaskState::Canceled | TaskState::Rejected
        )
    }

    /// Human-readable label for display contexts (TUI, logs).
    pub fn label(&self) -> &'static str {
        match self {
            TaskState::Unspecified => "unspecified",
            TaskState::Submitted => "submitted",
            TaskState::Working => "working",
            TaskState::InputRequired => "input-required",
            TaskState::Completed => "completed",
            TaskState::Failed => "failed",
            TaskState::Canceled => "canceled",
            TaskState::Rejected => "rejected",
            TaskState::AuthRequired => "auth-required",
        }
    }
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
            "TASK_STATE_UNSPECIFIED" => Ok(TaskState::Unspecified),
            "TASK_STATE_SUBMITTED" => Ok(TaskState::Submitted),
            "TASK_STATE_WORKING" => Ok(TaskState::Working),
            "TASK_STATE_INPUT_REQUIRED" => Ok(TaskState::InputRequired),
            "TASK_STATE_COMPLETED" => Ok(TaskState::Completed),
            "TASK_STATE_FAILED" => Ok(TaskState::Failed),
            "TASK_STATE_CANCELED" => Ok(TaskState::Canceled),
            "TASK_STATE_REJECTED" => Ok(TaskState::Rejected),
            "TASK_STATE_AUTH_REQUIRED" => Ok(TaskState::AuthRequired),
            _ => Err(format!("unknown task state: {s}")),
        }
    }
}
