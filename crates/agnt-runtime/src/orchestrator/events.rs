//! Event and result types flowing through the orchestrator.

use agnt_kernel::CompactionSource;
use nu_protocol::LabeledError;
use tokio::sync::mpsc;

use crate::protocol::event::PermissionDecisionSubmission;
use crate::protocol::{
    contracts::{McpUsabilityState, UiMessageSnapshot},
    persona::PersonaSummary,
};
use crate::session::SessionInfo;

pub type McpToggleResult = (
    Result<McpUsabilityState, String>,
    usize,
    usize,
    Vec<(String, Vec<String>)>,
);
pub type ModelSwitchResult = Result<(String, Option<u64>), String>;
pub type AgentSwitchResult = Result<(String, String, Option<u64>, Option<String>), String>;
pub type SessionSwitchResult = Result<Vec<UiMessageSnapshot>, String>;
pub type RefreshSessionPickerResult = Result<Vec<SessionInfo>, String>;
pub type RefreshAgentPickerResult = Result<Vec<PersonaSummary>, String>;

/// The surface's own reply hand for a picker catalog.
///
/// The catalog is surface state, never a broadcast: the surface that asked
/// owns the receiver and the stage answers on it. A request whose surface has
/// gone away carries no hand, and the catalog is simply dropped.
pub type PickerReply<T> = mpsc::Sender<T>;

/// Events flowing into the orchestrator loop from the TUI. The orchestrator
/// drains this channel with `while let Some` in a `select!` arm.
pub enum OrchestratorEvent {
    // ── From TUI (user actions) ──
    /// A prompt the user submitted. It carries no attribution: a prompt from
    /// the surface belongs to no external task, and the loop's own queue is the
    /// only road a task-attributed prompt travels.
    PromptSubmitted {
        text: String,
    },
    PermissionDecision {
        decision: PermissionDecisionSubmission,
    },
    UiRequest(UiRequest),

    // ── Signals ──
    CancelRequested,
    Quit,
    FatalError(LabeledError),
}

/// A request from the TUI that requires an async response from the worker.
#[derive(Clone)]
pub enum UiRequest {
    SwitchModel {
        spec: String,
    },
    SwitchAgent {
        name: String,
    },
    SwitchSession {
        id: String,
    },
    ToggleMcp {
        server: String,
        enable: bool,
    },
    RefreshSessionPicker {
        reply: Option<PickerReply<RefreshSessionPickerResult>>,
    },
    RefreshAgentPicker {
        reply: Option<PickerReply<RefreshAgentPickerResult>>,
    },
}

impl UiRequest {
    /// The words that name this request in a log line.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::SwitchModel { .. } => "model switch",
            Self::SwitchAgent { .. } => "agent switch",
            Self::SwitchSession { .. } => "session switch",
            Self::ToggleMcp { .. } => "MCP toggle",
            Self::RefreshSessionPicker { .. } => "session picker refresh",
            Self::RefreshAgentPicker { .. } => "agent picker refresh",
        }
    }
}

/// Per-type response to a `UiRequest`.
#[derive(Debug)]
pub enum UiRequestResponse {
    ModelSwitch(ModelSwitchResult),
    AgentSwitch(AgentSwitchResult),
    SessionSwitch {
        id: String,
        result: SessionSwitchResult,
    },
    McpToggle {
        server: String,
        result: Result<McpUsabilityState, String>,
        total: usize,
        server_count: usize,
        names_by_server: Vec<(String, Vec<String>)>,
    },
}

/// Commands dispatched to the worker task by the orchestrator loop.
pub enum WorkerCommand {
    ExecuteTurn {
        prompt: String,
        span: nu_protocol::Span,
        /// The A2A task that queued this prompt, or `None` for a user submit.
        ///
        /// The executor publishes it as the turn's `LifecycleEvent::TurnStarted`
        /// attribution, so the attribution travels with the command instead of
        /// being published by the stage that dispatched it.
        task_id: Option<String>,
    },
    HandleUiRequest {
        request: UiRequest,
        response_tx: mpsc::Sender<UiRequestResponse>,
    },
    /// Run a compaction the loop dispatched. Manual only: the automatic ladder
    /// completes its compaction on the turn itself and never reaches the worker.
    RunCompaction {
        source: CompactionSource,
    },
    /// Undo the newest compaction of the session.
    RunUncompact {
        source: CompactionSource,
    },
    /// Attach the session derived from an A2A `contextId`.
    ///
    /// The worker derives the session-store key as `{prefix}-{context_id}`
    /// (where `prefix` is the path-hash prefix of the working directory) and
    /// loads that session, creating it when it does not exist yet.
    ///
    /// `prompt` is the incoming A2A message text. The worker displays it on the
    /// UI bus after the attached session's transcript is hydrated, so the
    /// message never renders before the previous transcript is replaced.
    ///
    /// `task_id` is the task the message belongs to. The worker publishes the
    /// prompt row on the stream, stamped at this boundary, and hands the
    /// arrival to the loop as its intake command; the loop's queue attributes
    /// the turn.
    AttachA2aContext {
        context_id: String,
        prompt: String,
        task_id: String,
    },
    NewSession,
    Shutdown,
}
