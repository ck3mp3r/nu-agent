//! The runtime-owned external task vocabulary.
//!
//! An external task is work that arrives from outside this agent: the caller
//! already minted the conversation context and the prompt, so an
//! [`ExternalTask`] carries both. The agent only needs to dispatch the turn, or
//! report back through an [`ExternalTaskStore`] when it cannot.

// region:    --- Types

/// A task that arrived from an external caller.
///
/// The `context_id` is already resolved at the boundary that accepted the
/// task — the loop never mints one — so the loop attaches the session it names.
pub struct ExternalTask {
    /// Caller-assigned identifier of this task.
    pub task_id: String,
    /// Conversation context the turn belongs to.
    pub context_id: String,
    /// The prompt to run for this task.
    pub prompt: String,
}

/// A completion notification for a task this agent sent elsewhere.
///
/// The `task_id` names the REMOTE task, so it keys the notification on its way
/// to the prompt queue but never becomes a local attribution: the turn a
/// completion runs is the agent's own, not a turn for a task a caller handed
/// this agent.
pub struct ExternalCompletion {
    /// Identifier of the remote task that reached a terminal state.
    pub task_id: String,
    /// The prompt to run for this completion.
    pub prompt: String,
}

/// The rejection seam for external tasks.
///
/// A task whose caller transitioned it to a running state must reach a terminal
/// state even though no turn can run for it, so the loop reports the rejection
/// through this store instead of dropping the task.
pub trait ExternalTaskStore {
    /// Record `task_id` as rejected, with `reason` as the recorded cause.
    fn reject_task(&self, task_id: &str, reason: &str) -> core::result::Result<(), String>;
}

// endregion: --- Types

// region:    --- Implementations

/// The default store: no external task store is configured.
///
/// Used as the type-parameter default so loop construction sites that wire no
/// external task store keep compiling unchanged.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoExternalTaskStore;

impl ExternalTaskStore for NoExternalTaskStore {
    fn reject_task(&self, _task_id: &str, _reason: &str) -> core::result::Result<(), String> {
        Err("no external task store configured".to_string())
    }
}

// endregion: --- Implementations

// region:    --- Functions

/// Derive the internal session-store key for an external `context_id`.
///
/// The returned key is `{prefix}-{context_id}`, where `prefix` is the path-hash
/// prefix that scopes sessions to a working directory. The key format IS the
/// mapping — no registry is needed.
pub fn external_session_key(prefix: &str, context_id: &str) -> String {
    format!("{prefix}-{context_id}")
}

// endregion: --- Functions
