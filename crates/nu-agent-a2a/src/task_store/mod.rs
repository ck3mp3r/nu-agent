mod memory;
#[cfg(test)]
mod test;

pub use memory::InMemoryTaskStore;
pub use memory::is_valid_transition;

use std::collections::HashMap;

use serde_json::Value;
use tokio::sync::mpsc;

use crate::{A2aError, Artifact, Message, Task, TaskEvent, TaskState};

/// Backend contract for task storage. Implementors provide the actual
/// persistence mechanism (in-memory, SQLite, Redis, etc.).
///
/// Every method must be callable by production code — no dead trait methods.
pub trait TaskStoreBackend: Send + Sync {
    /// Create a new task in `Submitted` state.
    fn create_task(
        &self,
        session_id: Option<String>,
        context_id: Option<String>,
        parent_task_id: Option<String>,
        metadata: Option<HashMap<String, Value>>,
    ) -> Task;

    /// Retrieve a task by ID.
    fn get_task(&self, id: &str) -> Result<Task, A2aError>;

    /// Transition a task to a new state (with an optional message).
    fn update_status(
        &self,
        id: &str,
        new_state: TaskState,
        message: Option<Message>,
    ) -> Result<Task, A2aError>;

    /// Append an artifact to a task.
    fn add_artifact(&self, id: &str, artifact: Artifact) -> Result<Task, A2aError>;

    /// List tasks with optional filter by state, cursor-based pagination.
    ///
    /// Returns `(tasks, total_count, next_page_token)`.
    fn list_tasks(
        &self,
        filter: Option<Vec<TaskState>>,
        page_size: Option<usize>,
        next_page_token: Option<&str>,
    ) -> (Vec<Task>, usize, Option<String>);

    /// Subscribe to task events for a given task ID.
    fn subscribe(&self, id: &str) -> mpsc::Receiver<TaskEvent>;

    /// Remove a subscriber for a task.
    ///
    /// The default implementation is a no-op — cleanup happens implicitly when
    /// subscriber receivers are dropped (send errors trigger
    /// `prune_subscriptions` in `notify_*` methods).
    fn unregister_subscriber(&self, _id: &str, _rx: mpsc::Receiver<TaskEvent>) {
        // Default no-op.
        let _ = (_id, _rx);
    }

    /// Append a message to a task's history.
    fn append_history(&self, id: &str, msg: Message) -> Result<(), A2aError>;

    /// Create a task with an idempotency key (A2A spec §3.3.1).
    ///
    /// Returns `Ok(task)` for a new task or `Err((existing_task, sender))` if
    /// the key already exists (sender allows subscribing to the existing task).
    fn create_task_with_idempotency(
        &self,
        key: &str,
        session_id: Option<String>,
        context_id: Option<String>,
        parent_task_id: Option<String>,
        metadata: Option<HashMap<String, Value>>,
    ) -> Result<Task, Box<(Task, mpsc::Sender<TaskEvent>)>>;
}
