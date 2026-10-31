/// A request to cancel the current task.
#[derive(Debug, Clone)]
pub enum CancelEvent {
    Requested,
}

/// A process-wide agent lifecycle event.
///
/// Agent events are per-process, not per-session: one `Started`/`Shutdown` pair
/// per process lifetime. They ride a standalone channel created at mode entry,
/// never the per-runtime [`Bus`](crate::bus::Bus) — in ACP the bus is cloned per
/// session, so a process-scoped event on it would be a category error.
#[derive(Debug, Clone)]
pub enum AgentEvent {
    /// The agent process started. `mode` is "tui", "stderr", or "acp".
    Started { mode: String },
    /// The agent process is shutting down. `reason` is "completed", "error",
    /// or "shutdown".
    Shutdown { reason: String },
}

/// A conversation turn lifecycle event.
///
/// The turn's completion is not a `TurnEvent`: the executor publishes
/// `SystemEvent::TurnCompleted` on the ui-event channel, and the hooks runner
/// reads it there. This enum carries the control-plane facts only.
#[derive(Debug, Clone)]
pub enum TurnEvent {
    /// A turn started.
    Started {
        prompt: String,
        task_id: Option<String>,
    },
    /// An external (A2A) task completed with its output.
    TaskCompleted { output: String, task_id: String },
    /// An external (A2A) task failed or was cancelled, with the reason.
    TaskFailed { task_id: String, error: String },
}

/// A session lifecycle event.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    Started {
        session_id: String,
        hydrated: bool,
    },
    /// The session ended. Carries the session's cumulative LLM usage: the
    /// sums across every turn the session ran, with the same fallback and
    /// completeness conventions as [`SystemEvent::TurnCompleted`].
    ///
    /// [`SystemEvent::TurnCompleted`]: crate::protocol::events::SystemEvent::TurnCompleted
    Ended {
        session_id: String,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: Option<u64>,
        cost: Option<f64>,
        cost_complete: bool,
    },
    Switched {
        from_session_id: Option<String>,
        to_session_id: String,
    },
    /// The session's agent (persona) switched to another slug.
    ///
    /// `from` is the persona slug that was active, or `None` when no persona
    /// was loaded. `to` is the slug now active. The event rides the session
    /// channel, so a hook envelope is stamped from the session's own hook
    /// context — which the switch refreshes first.
    AgentSwitched {
        from: Option<String>,
        to: String,
    },
    /// The session's model switched to another identity.
    ///
    /// `from` is the `provider/model` identity in use before the switch; `to`
    /// is the identity now active. A model is always resolvable, so unlike
    /// [`SessionEvent::AgentSwitched`] there is no absent case.
    ModelSwitched {
        from: String,
        to: String,
    },
}

/// An external (e.g. A2A) event.
#[derive(Debug, Clone)]
pub enum ExternalEvent {
    PromptReceived {
        prompt: String,
        task_id: String,
        /// The A2A `contextId` this prompt belongs to, if known. Used for
        /// metadata/display; session routing is performed by the forwarder
        /// before the event is published.
        context_id: Option<String>,
    },
}
