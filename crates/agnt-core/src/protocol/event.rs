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
