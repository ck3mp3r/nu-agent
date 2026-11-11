pub use agnt_kernel::PermissionDecision;

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PermissionDecisionSubmission {
    pub request_id: String,
    pub decision: PermissionDecision,
}
