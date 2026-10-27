use crate::protocol::event::{ToolDisplay, UiEvent};
use crate::protocol::tool_args::CallLine;

/// A request to cancel the current task.
#[derive(Debug, Clone)]
pub enum CancelEvent {
    Requested,
}

/// A tool invocation lifecycle event.
#[derive(Debug, Clone)]
pub enum ToolEvent {
    Started {
        name: String,
        source: String,
        arguments: String,
        call_line: CallLine,
    },
    Completed {
        name: String,
        source: String,
        arguments: String,
        success: bool,
        result: String,
        display: Option<ToolDisplay>,
        error_kind: Option<String>,
        message: Option<String>,
    },
}

/// An LLM request lifecycle event.
#[derive(Debug, Clone)]
pub enum LlmEvent {
    Started,
    Completed {
        response_chars: usize,
        tool_calls: usize,
        input_tokens: u64,
        output_tokens: u64,
        /// The total the wire reported, or `None` when it reported none. A
        /// wire may omit it (ollama-cloud's OpenAI-compatible endpoint does),
        /// and an absent total is not a zero.
        total_tokens: Option<u64>,
        /// What the call cost in USD, when the provider reported it or the
        /// catalog priced it. `None` for a call nothing priced.
        cost: Option<f64>,
        /// Whether `cost` is the whole charge rather than a lower bound.
        cost_complete: bool,
    },
    AssistantMessage {
        text: String,
    },
    /// The run was stopped by a hook (e.g. a repetition/doom-loop stop). The
    /// reason is surfaced as an appended notice — never routed through the
    /// assistant-stream dedup path, so streamed content is not truncated.
    Stopped {
        reason: String,
    },
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
#[derive(Debug, Clone)]
pub enum TurnEvent {
    /// A turn started.
    Started {
        prompt: String,
        task_id: Option<String>,
    },
    /// A turn finished with the turn's tool-call count and LLM usage.
    ///
    /// The usage fields are the turn's sums across its LLM calls, accumulated
    /// publisher-side as each call completed. `total_tokens` is the sum of the
    /// per-call totals (each per the chain fallback: the wire's total, else
    /// input + output) and is `None` when no call reported one. `cost` is the
    /// sum of the reported per-call costs and is `None` when none was priced;
    /// `cost_complete` is the AND of the per-call flags, so a sum with one
    /// incomplete component is a lower bound. `duration_ms` is the turn's
    /// wall-clock span.
    Completed {
        tool_calls: usize,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: Option<u64>,
        cost: Option<f64>,
        cost_complete: bool,
        duration_ms: u64,
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
    /// completeness conventions as [`TurnEvent::Completed`].
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

/// A compaction lifecycle event.
#[derive(Debug, Clone)]
pub enum CompactionEvent {
    /// A compaction has been requested (auto-threshold or `/compact`). The
    /// orchestrator is the single subscriber that acts on this; the TUI does
    /// not render it.
    Requested {
        source: String,
    },
    Started {
        source: String,
    },
    SummaryChunk {
        source: String,
        delta: String,
        aggregated: String,
    },
    Completed {
        source: String,
        summary_preview: String,
        summary_body: String,
    },
    Failed {
        source: String,
        message: String,
    },
}

/// A warning message.
#[derive(Debug, Clone)]
pub enum WarningEvent {
    Message { message: String },
    TurnError { message: String },
}

/// A permission lifecycle event.
#[derive(Debug, Clone)]
pub enum PermissionEvent {
    Requested {
        request_id: String,
        context: Box<crate::protocol::event::PermissionRequestContext>,
    },
    DecisionSubmitted {
        request_id: String,
        decision: crate::protocol::event::PermissionDecision,
        matched_rule_identity: String,
    },
    DecisionTimedOut {
        request_id: String,
    },
    DecisionIgnored {
        request_id: String,
        reason: String,
    },
}

// region:    --- Froms

// These `From<XxxEvent> for Option<UiEvent>` impls are retained for the TUI
// state dispatch layer and for test code. The TUI reconstructs typed events
// from `UiEvent` fields in `dispatch_ui_event` (reducer.rs) and passes them to
// the state-level reducers; tests construct the typed enums directly.
// `From<CompactionEvent>` still has production callers, because compaction
// events ride their own `bus.compaction()` channel: the stderr drain converts
// them at the boundary (mode/stderr.rs) and the ACP translator converts them
// for the client's session/update notifications (translator.rs). The hooks
// runner (hooks/runner.rs) and the TUI (interactive.rs) consume
// `CompactionEvent` directly and need no conversion. Do not remove the others
// without first migrating the TUI state layer off the typed enums.

impl From<ToolEvent> for Option<UiEvent> {
    fn from(event: ToolEvent) -> Self {
        match event {
            ToolEvent::Started {
                name,
                source,
                arguments,
                call_line,
            } => Some(UiEvent::ToolStarted {
                name,
                source,
                arguments,
                call_line,
            }),
            ToolEvent::Completed {
                name,
                source,
                arguments,
                success,
                result,
                display,
                error_kind,
                message,
            } => Some(UiEvent::ToolCompleted {
                name,
                source,
                arguments,
                success,
                result,
                display,
                error_kind,
                message,
            }),
        }
    }
}

impl From<LlmEvent> for Option<UiEvent> {
    fn from(event: LlmEvent) -> Self {
        match event {
            LlmEvent::Started => Some(UiEvent::LlmStarted),
            LlmEvent::Completed {
                response_chars,
                tool_calls,
                input_tokens,
                output_tokens,
                total_tokens,
                cost,
                cost_complete,
            } => Some(UiEvent::LlmCompleted {
                response_chars,
                tool_calls,
                input_tokens,
                output_tokens,
                total_tokens,
                cost,
                cost_complete,
            }),
            LlmEvent::AssistantMessage { text } => Some(UiEvent::AssistantMessage {
                delta: text.clone(),
                text,
            }),
            LlmEvent::Stopped { reason } => Some(UiEvent::Stopped { reason }),
        }
    }
}

impl From<WarningEvent> for Option<UiEvent> {
    fn from(event: WarningEvent) -> Self {
        match event {
            WarningEvent::Message { message } => Some(UiEvent::Warning { message }),
            WarningEvent::TurnError { message } => Some(UiEvent::TurnError { message }),
        }
    }
}

impl From<CompactionEvent> for Option<UiEvent> {
    fn from(event: CompactionEvent) -> Self {
        match event {
            CompactionEvent::Requested { .. } => None,
            CompactionEvent::Started { source } => Some(UiEvent::CompactionStarted { source }),
            CompactionEvent::SummaryChunk {
                source,
                delta,
                aggregated,
            } => Some(UiEvent::CompactionSummaryChunk {
                source,
                delta,
                aggregated,
            }),
            CompactionEvent::Completed {
                source,
                summary_preview,
                summary_body,
            } => Some(UiEvent::CompactionCompleted {
                source,
                summary_preview,
                summary_body,
            }),
            CompactionEvent::Failed { source, message } => {
                Some(UiEvent::CompactionFailed { source, message })
            }
        }
    }
}

impl From<TurnEvent> for Option<UiEvent> {
    fn from(event: TurnEvent) -> Self {
        match event {
            // Turn started is not rendered in the TUI.
            TurnEvent::Started { .. } => None,
            TurnEvent::Completed {
                tool_calls,
                input_tokens,
                output_tokens,
                total_tokens,
                cost,
                cost_complete,
                duration_ms,
            } => Some(UiEvent::Completed {
                tool_calls,
                input_tokens,
                output_tokens,
                total_tokens,
                cost,
                cost_complete,
                duration_ms,
            }),
            // A2A-only completion — not for the TUI.
            TurnEvent::TaskCompleted { .. } => None,
            // A2A-only failure — not for the TUI.
            TurnEvent::TaskFailed { .. } => None,
        }
    }
}

impl From<SessionEvent> for Option<UiEvent> {
    fn from(_event: SessionEvent) -> Self {
        // Session lifecycle events are not rendered in the TUI.
        None
    }
}

impl From<PermissionEvent> for Option<UiEvent> {
    fn from(event: PermissionEvent) -> Self {
        match event {
            PermissionEvent::Requested {
                request_id,
                context,
            } => Some(UiEvent::PermissionRequested {
                request_id,
                context: *context,
            }),
            PermissionEvent::DecisionSubmitted {
                request_id,
                decision,
                matched_rule_identity,
            } => Some(UiEvent::PermissionDecisionSubmitted {
                request_id,
                decision,
                matched_rule_identity,
            }),
            PermissionEvent::DecisionTimedOut { request_id } => {
                Some(UiEvent::PermissionDecisionTimedOut { request_id })
            }
            PermissionEvent::DecisionIgnored { request_id, reason } => {
                Some(UiEvent::PermissionDecisionIgnored { request_id, reason })
            }
        }
    }
}
