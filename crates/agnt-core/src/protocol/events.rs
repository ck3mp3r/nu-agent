//! The one typed event vocabulary for a session.
//!
//! `SystemEvent` is the model-update message of the transcript state fold. The
//! variant list comes from the fact inventory in design note `6ad10855` §3.1
//! rev 7: every fact the pre-wave `UiEvent`, the bus event enums, and
//! `UserAction` carried appears here exactly once, with typed payloads. Those
//! three vocabularies are deleted; this enum replaced them.

use crate::protocol::event::{PermissionDecision, PermissionRequestContext, ToolDisplay};
use crate::protocol::tool_args::CallLine;
use crate::session::tool_status::ToolStatus;
use crate::transcript::ir::ToolName;

// region:    --- Types

/// Everything that happens to a session, as one typed vocabulary.
///
/// The reducer folds these events into state. Nothing else writes state and
/// no consumer sniffs rendered strings.
#[derive(Debug, Clone, PartialEq)]
pub enum SystemEvent {
    // user — input-boundary facts
    /// A key the reducer interprets. The terminal layer maps raw keys here.
    KeyPressed { key: Key },
    /// A mouse-wheel scroll at a terminal cell.
    WheelScrolled { up: bool, column: u16, row: u16 },
    /// The terminal was resized.
    Resized { width: u16, height: u16 },
    /// A permission card decision. The reducer resolves the active request id.
    PermissionDecided { decision: PermissionDecision },
    /// The user submitted prompt text. The reducer parses slash commands.
    PromptSubmitted { text: String },

    // inference
    /// An inference call started.
    InferenceStarted,
    /// One streamed delta of the inference response.
    ///
    /// `delta` is the newly received text. `aggregated` is the accumulated
    /// response text so far. A non-streaming producer emits one chunk with
    /// both fields equal to the full text.
    InferenceChunk { delta: String, aggregated: String },
    /// The inference call finished; `usage` carries every reported total.
    InferenceCompleted { usage: Usage },
    /// A hook stopped the run.
    InferenceStopped { reason: String },

    // tools
    /// A tool call started.
    ToolStarted { call: ToolCall },
    /// The permission gate produced a preview for a tool call.
    PreviewRequested {
        call: ToolCall,
        display: ToolDisplay,
    },
    /// A tool call finished.
    ToolCompleted {
        call: ToolCall,
        outcome: ToolOutcome,
    },

    // permission lifecycle
    /// A permission prompt was opened.
    PermissionRequested {
        request_id: RequestId,
        context: PermissionRequestContext,
    },
    /// A decision was submitted for a request.
    PermissionDecisionSubmitted {
        request_id: RequestId,
        decision: PermissionDecision,
        matched_rule_identity: String,
    },
    /// A request expired without a decision.
    PermissionDecisionTimedOut { request_id: RequestId },
    /// A submitted decision was discarded.
    PermissionDecisionIgnored {
        request_id: RequestId,
        reason: String,
    },
    /// A pending request was abandoned because the turn was cancelled.
    PermissionRequestCancelled {
        request_id: RequestId,
        matched_rule_identity: String,
    },
    /// The policy resolved a call without prompting the user.
    PermissionAutoResolved {
        matched_rule_identity: String,
        action: AutoResolutionAction,
        reason: Option<String>,
    },

    // system
    /// A system message shown to the user.
    SystemMessage { kind: SystemKind, text: String },
    /// Compaction was decided for a turn.
    CompactionRequested { source: String },
    /// Compaction started.
    CompactionStarted { source: String },
    /// One streamed delta of the compaction summary.
    CompactionSummaryChunk {
        source: String,
        delta: String,
        aggregated: String,
    },
    /// Compaction finished.
    CompactionCompleted {
        source: String,
        summary_preview: String,
        summary_body: String,
    },
    /// Compaction failed.
    CompactionFailed { source: String, message: String },

    // turn
    /// The turn finished; the totals sum across its calls.
    TurnCompleted {
        tool_calls: usize,
        input_tokens: u64,
        output_tokens: u64,
        /// The total the wire reported, or `None` when it reported none.
        total_tokens: Option<u64>,
        /// What the call cost in USD, when anything priced it.
        cost: Option<f64>,
        /// Whether `cost` is the whole charge rather than a lower bound.
        cost_complete: bool,
        duration_ms: u64,
    },

    // environment
    /// The idle tick woke the render loop.
    Tick,
}

/// The exact key of a tool call: `{tool_name}\n{raw_arguments}`.
///
/// One constructor derives it, so the call key cannot drift between the
/// permission path, the hook chain, and the transcript.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ToolCallId(String);

/// One tool call, as the transcript and the permission gate identify it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolCall {
    pub id: ToolCallId,
    pub name: ToolName,
    pub source: String,
    /// The raw arguments JSON, the other half of the call key.
    pub arguments: String,
    pub call_line: CallLine,
}

/// The result of a finished tool call.
///
/// `status` is the structural classification the transcript renders by; the
/// other fields are the call's content and the handler's own vocabulary, which
/// never substitute for the status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolOutcome {
    pub success: bool,
    pub status: ToolStatus,
    pub result: String,
    pub display: Option<ToolDisplay>,
    pub error_kind: Option<String>,
    pub message: Option<String>,
}

/// Identity of one permission request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestId(String);

/// The usage of one inference call and the totals of a completed turn.
#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    pub response_chars: usize,
    pub tool_calls: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// The total the wire reported, or `None` when it reported none.
    pub total_tokens: Option<u64>,
    /// What this call cost in USD, when anything priced it.
    pub cost: Option<f64>,
    /// Whether `cost` is the whole charge rather than a lower bound.
    pub cost_complete: bool,
}

/// A key the reducer interprets. Core owns this enum so no terminal
/// library type crosses the crate boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Enter,
    AltEnter,
    ShiftEnter,
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
    Up,
    Down,
    PageUp,
    PageDown,
    CtrlU,
    CtrlD,
    CtrlP,
    CtrlN,
    CtrlJ,
    CtrlK,
    Tab,
    BackTab,
    Esc,
    CtrlC,
}

/// What kind of system message this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemKind {
    Warning,
    TurnError,
}

/// The action a policy resolved a tool call to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoResolutionAction {
    Allow,
    Deny,
}

// endregion: --- Types

// region:    --- Impls

impl ToolCall {
    /// Build a call and derive its key from the name and the raw arguments.
    pub fn new(
        name: ToolName,
        source: impl Into<String>,
        arguments: impl Into<String>,
        call_line: CallLine,
    ) -> Self {
        let arguments = arguments.into();
        let id = ToolCallId::of(&name.0, &arguments);
        Self {
            id,
            name,
            source: source.into(),
            arguments,
            call_line,
        }
    }
}

impl ToolCallId {
    /// Derive the key of a call from its tool name and raw arguments.
    pub fn of(tool_name: &str, raw_arguments: &str) -> Self {
        Self(format!("{tool_name}\n{raw_arguments}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl RequestId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AutoResolutionAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Allow => "allow",
            Self::Deny => "deny",
        }
    }
}

// endregion: --- Impls
