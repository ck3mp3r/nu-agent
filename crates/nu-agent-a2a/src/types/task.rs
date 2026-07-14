use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::artifact::Artifact;
use super::message::Message;
use super::task_state::TaskState;
use super::task_status::TaskStatus;

// ---------------------------------------------------------------------------
// IncomingTask — event channel type for received tasks
// ---------------------------------------------------------------------------

/// An incoming task received from a remote A2A agent via the HTTP API.
///
/// This is delivered to consumers via the event channel obtained from
/// [`A2aServer::take_incoming_task_receiver`].
#[derive(Clone, Debug)]
pub struct IncomingTask {
    /// The server-assigned UUID of the task.
    pub task_id: String,
    /// The deserialised user message.
    pub message: Message,
    /// The `senderUrl` sent by the remote agent (may be empty).
    pub sender_url: String,
    /// Optional session identifier.
    pub session_id: Option<String>,
    /// Optional multi-turn conversation context identifier.
    pub context_id: Option<String>,
    /// Optional identifier of the parent task that spawned this one.
    pub parent_task_id: Option<String>,
}

// ---------------------------------------------------------------------------
// A2aCompletionEvent
// ---------------------------------------------------------------------------

/// A completion event delivered when a remote agent finishes processing
/// a task that was sent via `tasks.send`.
///
/// This is produced by a background SSE watcher and delivered to the agent
/// runtime via a shared channel, so the LLM sees a completion message on
/// the next turn without having to poll.
#[derive(Clone, Debug)]
pub struct A2aCompletionEvent {
    pub task_id: String,
    pub agent_name: String,
    /// Concatenated text parts from the final task result text (from
    /// artifacts or status message).
    pub result: String,
    pub status: TaskState,
}

// ---------------------------------------------------------------------------
// TaskEvent — notification channel event for live subscribers
// ---------------------------------------------------------------------------

/// An event sent to subscribed SSE clients when a task's status changes or
/// an artifact is added.
#[derive(Clone, Debug, Serialize)]
pub enum TaskEvent {
    StatusChanged { task_id: String, status: TaskStatus },
    ArtifactAdded { task_id: String, artifact: Artifact },
}

// ---------------------------------------------------------------------------
// Task (A2A spec §9.3)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    #[serde(rename = "contextId", skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    #[serde(rename = "parentTaskId", skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    #[serde(rename = "sessionId", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub status: TaskStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history: Option<Vec<Message>>,
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, Value>>,
}
