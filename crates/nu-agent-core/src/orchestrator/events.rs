//! Event and result types flowing through the orchestrator.

use nu_protocol::LabeledError;
use tokio::sync::mpsc;

use crate::protocol::{
    contracts::{McpUsabilityState, SharedUiAction, UiMessageSnapshot},
    event::PermissionDecisionSubmission,
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

/// Events flowing into the orchestrator loop from the TUI. The orchestrator
/// drains this channel with `while let Some` in a `select!` arm.
pub enum OrchestratorEvent {
    // ── From TUI (user actions) ──
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
    SwitchModel { spec: String },
    SwitchAgent { name: String },
    SwitchSession { id: String },
    ToggleMcp { server: String, enable: bool },
    RefreshSessionPicker,
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
    SessionRefresh(RefreshSessionPickerResult),
}

/// UI state updates broadcast to the TUI render loop.
#[derive(Clone)]
pub enum UiStateEvent {
    SetActiveModelIdentity(String),
    SetActiveAgentIdentity(String),
    SetActivePersonaIcon(Option<String>),
    SetContextWindowMaxTokens(Option<u64>),
    ClearTranscript,
    HydrateTranscript {
        messages: Vec<UiMessageSnapshot>,
        last_total_tokens: Option<u64>,
    },
    SetMcpServerState {
        server: String,
        state: McpUsabilityState,
        error: Option<String>,
        total: usize,
    },
    SetMcpVisibleToolCount {
        server: String,
        count: usize,
    },
    SetMcpVisibleToolNames {
        server: String,
        names: Vec<String>,
    },
    SetSessionPickerOptions(Vec<SessionInfo>),
    EnqueueExternalPrompt {
        text: String,
    },
    ExecuteSharedUiAction(SharedUiAction),
    PushStartupLogo,
}

/// Commands dispatched to the worker task by the orchestrator loop.
pub enum WorkerCommand {
    ExecuteTurn {
        prompt: String,
        span: nu_protocol::Span,
    },
    HandleUiRequest {
        request: UiRequest,
        response_tx: mpsc::Sender<UiRequestResponse>,
    },
    RunCompaction {
        source: String,
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
    AttachA2aContext {
        context_id: String,
        prompt: String,
    },
    ClearSession,
    NewSession,
    Shutdown,
}
