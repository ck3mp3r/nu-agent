#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDisplay {
    pub title: String,
    pub sections: Vec<ToolDisplaySection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolDisplaySection {
    pub label: String,
    pub language: String,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiEvent {
    LlmStart,
    Tick,
    LlmEnd {
        response_chars: usize,
        tool_calls: usize,
        input_tokens: u64,
        output_tokens: u64,
        total_tokens: u64,
    },
    ToolStart {
        name: String,
        source: String,
        arguments: String,
    },
    ToolEnd {
        name: String,
        source: String,
        arguments: String,
        success: bool,
        result: String,
        display: Option<ToolDisplay>,
        error_kind: Option<String>,
        message: Option<String>,
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
    CompactionTriggered {
        source: String,
        summarized_count: usize,
        kept_recent_count: usize,
        summary_preview: String,
        summary_body: String,
    },
    CompactionFailed {
        source: String,
        message: String,
    },
    AssistantMessage {
        text: String,
    },
    Completed {
        tool_calls: usize,
    },
}
