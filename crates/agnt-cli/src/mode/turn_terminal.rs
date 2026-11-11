//! The one turn→terminal mapping: the store transition a finished turn reaches,
//! and the task-terminal row it publishes.

use agnt_a2a::InMemoryTaskStore;
use agnt_runtime::bus::{Lifecycle, LifecycleEvent};
use agnt_runtime::utils::value_ext::extract_response_text_from_value;
use nu_protocol::Value;

use crate::error::Result;

// region:    --- Types

/// The terminal state one turn reaches for the task it ran.
///
/// The single vocabulary every turn→terminal writer shares. The spine listener
/// maps a task-terminal row to one of these; the in-hand stderr path maps its
/// one-shot result to one. Both then take the same road through
/// [`apply_turn_terminal`], so the store transition exists once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskTerminal {
    /// The turn finished; `output` is the response text.
    Completed { output: String },
    /// The turn failed or was cancelled; `error` is the reason text.
    Failed { error: String },
}

// endregion: --- Types

// region:    --- Functions

/// The one turn→terminal mapping.
///
/// Transitions `task_id` to the terminal state its turn reached, then publishes
/// the matching task-terminal row on `lifecycle` so every observer attributes
/// the outcome. Store and publish failures are logged, never propagated: a
/// write-back failure must not fail the turn.
///
/// Its two drivers are the TUI's spine listener and stderr's in-hand path. The
/// listener does not call this function — it already consumed the row this
/// would publish — it calls [`TaskTerminal::apply_to_store`], the transition
/// this wraps.
pub fn apply_turn_terminal(
    store: &InMemoryTaskStore,
    lifecycle: &Lifecycle,
    task_id: &str,
    terminal: TaskTerminal,
) {
    terminal.apply_to_store(store, task_id);
    if let Err(e) = lifecycle.publish(terminal.lifecycle_row(task_id)) {
        log::warn!("Failed to publish the task-terminal row for {task_id}: {e}");
    }
}

/// Apply a task-terminal lifecycle row to the A2A task store.
///
/// The TUI listener's driver: it maps the row it consumed to a [`TaskTerminal`]
/// and takes the same transition stderr takes. Every other variant is ignored.
pub fn apply_turn_event_to_store(store: &InMemoryTaskStore, event: &LifecycleEvent) {
    let Some((task_id, terminal)) = TaskTerminal::from_lifecycle_row(event) else {
        return;
    };
    terminal.apply_to_store(store, task_id);
}

// endregion: --- Functions

// region:    --- TaskTerminal

impl TaskTerminal {
    // --- Constructors ---

    /// The terminal a one-shot turn result reaches.
    ///
    /// A success completes with the response text; any error fails with its
    /// message.
    pub fn from_result(result: &Result<Value>) -> Self {
        match result {
            Ok(response) => Self::Completed {
                output: extract_response_text_from_value(response),
            },
            Err(error) => Self::Failed {
                error: error.to_string(),
            },
        }
    }

    /// The terminal a task-terminal spine row reports, with the task it names.
    ///
    /// `None` for every lifecycle record that is not a task terminal.
    pub fn from_lifecycle_row(event: &LifecycleEvent) -> Option<(&str, Self)> {
        match event {
            LifecycleEvent::TaskCompleted { task_id, output } => Some((
                task_id.as_str(),
                Self::Completed {
                    output: output.clone(),
                },
            )),
            LifecycleEvent::TaskFailed { task_id, error } => Some((
                task_id.as_str(),
                Self::Failed {
                    error: error.clone(),
                },
            )),
            _ => None,
        }
    }

    // --- Accessors ---

    /// The spine row this terminal publishes for `task_id`.
    pub fn lifecycle_row(&self, task_id: &str) -> LifecycleEvent {
        match self {
            Self::Completed { output } => LifecycleEvent::TaskCompleted {
                task_id: task_id.to_string(),
                output: output.clone(),
            },
            Self::Failed { error } => LifecycleEvent::TaskFailed {
                task_id: task_id.to_string(),
                error: error.clone(),
            },
        }
    }

    // --- Methods ---

    /// Transition `task_id` to this terminal state in `store`.
    ///
    /// Store errors are logged, never propagated: an invalid transition means
    /// another driver already reached the terminal, which is benign.
    pub fn apply_to_store(&self, store: &InMemoryTaskStore, task_id: &str) {
        let transition = match self {
            Self::Completed { output } => store.complete_task(task_id, output),
            Self::Failed { error } => store.fail_task(task_id, error),
        };
        if let Err(e) = transition {
            log::warn!("Failed to apply the turn terminal to A2A task {task_id}: {e}");
        }
    }
}

// endregion: --- TaskTerminal
