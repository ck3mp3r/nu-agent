use crate::protocol::tool_args::CallLine;
use crate::transcript::ir::ContentKind;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDisplay {
    pub title: Option<String>,
    pub sections: Vec<ToolDisplaySection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDisplaySection {
    pub label: Option<String>,
    pub kind: ContentKind,
    pub content: String,
    pub stats: Option<ToolDisplayStats>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolDisplayStats {
    pub files_changed: Option<usize>,
    pub insertions: Option<usize>,
    pub deletions: Option<usize>,
    pub diff_truncated: Option<bool>,
    pub omitted_files: Option<usize>,
    pub omitted_hunks: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    AllowOnce,
    AllowAlways,
    Deny,
}

impl PermissionDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllowOnce => "allow_once",
            Self::AllowAlways => "allow_always",
            Self::Deny => "deny",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionRequestContext {
    pub tool: String,
    /// Exact tool-call key (`{tool_name}\n{raw_arguments}`) for the call this
    /// request refers to. `tool` is a decorated display name and cannot be
    /// matched against call keys; this field carries the key as data.
    pub tool_key: String,
    pub source: String,
    pub mode: Option<String>,
    pub matched_rule_identity: String,
    pub scope: String,
    pub target_field: Option<String>,
    pub pattern: String,
    pub summary: String,
    pub pre_authorize_display: Option<ToolDisplay>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDecisionSubmission {
    pub request_id: String,
    pub decision: PermissionDecision,
    pub matched_rule_identity: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UiEvent {
    LlmStarted,
    Tick,
    LlmCompleted {
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
    ToolStarted {
        name: String,
        source: String,
        arguments: String,
        call_line: CallLine,
    },
    ToolCompleted {
        name: String,
        source: String,
        arguments: String,
        success: bool,
        result: String,
        display: Option<ToolDisplay>,
        error_kind: Option<String>,
        message: Option<String>,
    },
    /// A pre-authorize preview published without a permission prompt (the
    /// policy auto-approved the call). `tool_key` is the exact
    /// `{tool_name}\n{raw_arguments}` call key, the same contract as
    /// [`PermissionRequestContext::tool_key`].
    ToolPreview {
        tool_key: String,
        display: ToolDisplay,
    },
    PermissionRequested {
        request_id: String,
        context: PermissionRequestContext,
    },
    PermissionDecisionSubmitted {
        request_id: String,
        decision: PermissionDecision,
        matched_rule_identity: String,
    },
    PermissionDecisionTimedOut {
        request_id: String,
    },
    PermissionDecisionIgnored {
        request_id: String,
        reason: String,
    },
    /// A pending permission request was abandoned because the turn was
    /// cancelled. No decision exists at that moment, so the payload carries
    /// the request id and the identity of the rule that produced the prompt.
    PermissionRequestCancelled {
        request_id: String,
        matched_rule_identity: String,
    },
    /// The policy resolved a tool call without prompting the user.
    ///
    /// `action` is `"allow"` or `"deny"`. `reason` is `Some` only when
    /// `action` is `"deny"`, and carries the rule-derived denial reason.
    ///
    /// The payload carries no tool identity: `ToolStarted` precedes this event
    /// on the same channel, so a hook correlates by order or subscribes to both.
    PermissionAutoResolved {
        matched_rule_identity: String,
        action: String,
        reason: Option<String>,
    },
    Warning {
        message: String,
    },
    /// A turn-level error that should be prominently displayed
    TurnError {
        message: String,
    },
    CompactionStarted {
        source: String,
    },
    CompactionSummaryChunk {
        source: String,
        delta: String,
        aggregated: String,
    },
    CompactionCompleted {
        source: String,
        summary_preview: String,
        summary_body: String,
    },
    CompactionFailed {
        source: String,
        message: String,
    },
    AssistantMessage {
        delta: String,
        text: String,
    },
    /// A hook stopped the run; the reason is appended as a closing notice.
    Stopped {
        reason: String,
    },
    /// The turn finished. Carries the turn's tool-call count and its LLM
    /// usage, so a consumer that reconstructs the typed event from this one
    /// loses nothing: the usage fields are the turn's sums across its calls
    /// (see [`crate::bus::TurnEvent::Completed`]).
    Completed {
        tool_calls: usize,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: Option<u64>,
        cost: Option<f64>,
        cost_complete: bool,
        duration_ms: u64,
    },
}
