use crate::transcript::ir::ContentKind;
use serde::{Deserialize, Serialize};

/// A tool display embedded in a persisted tool result. Serialized with the
/// `kind` discriminator its sections carry, so the payload round-trips into
/// [`ToolDisplay`] without a second shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDisplay {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub sections: Vec<ToolDisplaySection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDisplaySection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(flatten)]
    pub kind: ContentKind,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<ToolDisplayStats>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ToolDisplayStats {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub files_changed: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub insertions: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_truncated: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub omitted_files: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
