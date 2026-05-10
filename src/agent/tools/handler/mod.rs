use nu_plugin::EngineInterface;
use nu_protocol::{Span, Value, shell_error::generic::GenericError};
use rig::completion::message::{AssistantContent, ToolCall};
use serde_json::Value as JsonValue;

#[cfg(test)]
use std::cell::RefCell;

use crate::agent::protocol::event::{ToolDisplay, ToolDisplaySection, ToolDisplayStats};

use crate::tools::{closure::ClosureRegistry, error::ToolError, executor::ToolExecutor};

#[derive(Debug, serde::Deserialize)]
struct BuiltinReadArgs {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
struct BuiltinEditArgs {
    path: String,
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    replacement: Option<String>,
    #[serde(default)]
    expected_version: Option<String>,
    #[serde(default)]
    match_mode: Option<String>,
    #[serde(default)]
    occurrence: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    operation: Option<BuiltinEditOperationArgs>,
}

#[derive(Debug, serde::Deserialize, Clone)]
struct BuiltinEditOperationArgs {
    #[serde(default)]
    #[serde(rename = "type")]
    operation_type: Option<String>,
    search: String,
    replacement: String,
    #[serde(default)]
    match_mode: Option<String>,
    #[serde(default)]
    occurrence: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct BuiltinPatchRangeArgs {
    start: usize,
    end: usize,
}

#[derive(Debug, serde::Deserialize)]
struct BuiltinPatchOpArgs {
    range: BuiltinPatchRangeArgs,
    replacement: String,
}

#[derive(Debug, serde::Deserialize)]
struct BuiltinPatchArgs {
    path: String,
    #[serde(default)]
    expected_version: Option<String>,
    operations: Vec<BuiltinPatchOpArgs>,
}

#[derive(Debug, serde::Deserialize)]
struct BuiltinSkillArgs {
    name: String,
}

#[derive(Debug, Clone)]
struct BuiltinFsToolError {
    kind: ToolErrorKind,
    message: String,
    details: Option<JsonValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolSource {
    Closure,
    Mcp,
    Unknown,
}

impl ToolSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Closure => "closure",
            Self::Mcp => "mcp",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolErrorKind {
    Timeout,
    Validation,
    Runtime,
    Transport,
    Unknown,
}

impl ToolErrorKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::Validation => "validation",
            Self::Runtime => "runtime",
            Self::Transport => "transport",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ToolFailureOutcome {
    pub tool_name: String,
    pub tool_call_id: String,
    pub source: ToolSource,
    pub error_kind: ToolErrorKind,
    pub message: String,
    pub details: Option<JsonValue>,
}

impl ToolFailureOutcome {
    pub fn to_json_value(&self) -> JsonValue {
        let mut obj = serde_json::Map::new();
        obj.insert(
            "tool_name".to_string(),
            JsonValue::String(self.tool_name.clone()),
        );
        obj.insert(
            "tool_call_id".to_string(),
            JsonValue::String(self.tool_call_id.clone()),
        );
        obj.insert(
            "source".to_string(),
            JsonValue::String(self.source.as_str().to_string()),
        );
        obj.insert(
            "error_kind".to_string(),
            JsonValue::String(self.error_kind.as_str().to_string()),
        );
        obj.insert(
            "message".to_string(),
            JsonValue::String(self.message.clone()),
        );

        if let Some(details) = &self.details {
            obj.insert("details".to_string(), details.clone());
        }

        JsonValue::Object(obj)
    }

    fn to_json_string(&self) -> String {
        serde_json::to_string(&self.to_json_value()).unwrap_or_else(|_| {
            format!(
                r#"{{"tool_name":"{}","tool_call_id":"{}","source":"{}","error_kind":"{}","message":"{}"}}"#,
                self.tool_name,
                self.tool_call_id,
                self.source.as_str(),
                self.error_kind.as_str(),
                self.message
            )
        })
    }
}

#[derive(Debug, Clone)]
pub struct McpToolRegistry {
    names: std::collections::HashSet<String>,
    raw_name_by_exposed_name: std::collections::HashMap<String, String>,
    server_by_exposed_name: std::collections::HashMap<String, String>,
    enabled_servers: std::sync::Arc<std::sync::RwLock<std::collections::HashSet<String>>>,
}

impl McpToolRegistry {
    #[cfg(test)]
    pub fn from_names<I, S>(names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let names: std::collections::HashSet<String> = names.into_iter().map(Into::into).collect();
        let raw_name_by_exposed_name = names
            .iter()
            .map(|name| (name.clone(), name.clone()))
            .collect::<std::collections::HashMap<_, _>>();
        let server_by_exposed_name = names
            .iter()
            .map(|name| (name.clone(), name.clone()))
            .collect::<std::collections::HashMap<_, _>>();
        let enabled_servers = names.iter().cloned().collect();
        Self {
            raw_name_by_exposed_name,
            server_by_exposed_name,
            enabled_servers: std::sync::Arc::new(std::sync::RwLock::new(enabled_servers)),
            names,
        }
    }

    pub fn from_tools<I>(tools: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = crate::tools::mcp::client::McpToolDefinition>,
    {
        let mut names = std::collections::HashSet::new();
        let mut raw_name_by_exposed_name = std::collections::HashMap::new();
        let mut server_by_exposed_name = std::collections::HashMap::new();
        let mut enabled_servers = std::collections::HashSet::new();

        for tool in tools {
            let exposed_name = tool.name;
            let raw_name = tool.raw_name;
            let server_name = tool.server;
            if !names.insert(exposed_name.clone()) {
                return Err(format!(
                    "duplicate exposed MCP tool name '{}' while building MCP registry",
                    exposed_name
                ));
            }
            raw_name_by_exposed_name.insert(exposed_name.clone(), raw_name);
            server_by_exposed_name.insert(exposed_name, server_name.clone());
            enabled_servers.insert(server_name);
        }

        Ok(Self {
            names,
            raw_name_by_exposed_name,
            server_by_exposed_name,
            enabled_servers: std::sync::Arc::new(std::sync::RwLock::new(enabled_servers)),
        })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.contains(name) && self.is_tool_enabled(name)
    }

    pub fn is_registered(&self, name: &str) -> bool {
        self.names.contains(name)
    }

    pub fn raw_name_for(&self, exposed_name: &str) -> Option<&str> {
        self.raw_name_by_exposed_name
            .get(exposed_name)
            .map(String::as_str)
    }

    pub fn is_tool_enabled(&self, exposed_name: &str) -> bool {
        let Some(server_name) = self.server_by_exposed_name.get(exposed_name) else {
            return false;
        };

        self.enabled_servers
            .read()
            .map(|servers| servers.contains(server_name))
            .unwrap_or(false)
    }

    pub fn set_server_enabled(&self, server_name: &str, enabled: bool) -> Result<(), String> {
        let mut servers = self
            .enabled_servers
            .write()
            .map_err(|_| "MCP enabled-server state lock poisoned".to_string())?;

        if enabled {
            servers.insert(server_name.to_string());
        } else {
            servers.remove(server_name);
        }

        Ok(())
    }

    pub fn is_server_enabled(&self, server_name: &str) -> bool {
        self.enabled_servers
            .read()
            .map(|servers| servers.contains(server_name))
            .unwrap_or(false)
    }
}

fn resolve_mcp_invocation_name<'a>(
    registry: &'a McpToolRegistry,
    exposed_tool_name: &str,
) -> Option<&'a str> {
    registry.raw_name_for(exposed_tool_name)
}

pub(crate) fn llm_visible_tool_definitions(
    tool_definitions: &[rig::completion::ToolDefinition],
    mcp_registry: &McpToolRegistry,
) -> Vec<rig::completion::ToolDefinition> {
    tool_definitions
        .iter()
        .filter(|tool| {
            if mcp_registry.is_registered(tool.name.as_str()) {
                mcp_registry.contains(tool.name.as_str())
            } else {
                true
            }
        })
        .cloned()
        .collect()
}

fn classify_tool_source(
    tool_name: &str,
    closure_registry: &ClosureRegistry,
    mcp_registry: &McpToolRegistry,
) -> Option<ToolSource> {
    if closure_registry.get(tool_name).is_some() || is_builtin_fs_tool_name(tool_name) {
        Some(ToolSource::Closure)
    } else if mcp_registry.contains(tool_name) {
        Some(ToolSource::Mcp)
    } else {
        None
    }
}

pub(crate) fn is_builtin_fs_tool_name(tool_name: &str) -> bool {
    matches!(tool_name, "read" | "edit" | "patch" | "skill")
}

fn parse_edit_match_mode(value: Option<&str>) -> Result<crate::tools::fs::core::EditMatchMode, BuiltinFsToolError> {
    match value.unwrap_or("literal") {
        "literal" => Ok(crate::tools::fs::core::EditMatchMode::Literal),
        "regex" => Ok(crate::tools::fs::core::EditMatchMode::Regex),
        other => Err(BuiltinFsToolError {
            kind: ToolErrorKind::Validation,
            message: format!("Invalid edit.match_mode '{other}': expected 'literal' or 'regex'"),
            details: None,
        }),
    }
}

fn parse_edit_occurrence(value: Option<&str>) -> Result<crate::tools::fs::core::EditOccurrence, BuiltinFsToolError> {
    match value.unwrap_or("first") {
        "first" => Ok(crate::tools::fs::core::EditOccurrence::First),
        "all" => Ok(crate::tools::fs::core::EditOccurrence::All),
        other => Err(BuiltinFsToolError {
            kind: ToolErrorKind::Validation,
            message: format!("Invalid edit.occurrence '{other}': expected 'first' or 'all'"),
            details: None,
        }),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditToolMode {
    Preview,
    Apply,
}

impl EditToolMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Preview => "preview",
            Self::Apply => "apply",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
enum EditWriteDecision {
    Approve,
    Deny { message: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EditWritePolicy {
    AutoApprove,
}

impl EditWritePolicy {
    fn decide(self, _path: &std::path::Path, _plan: &crate::tools::fs::core::EditPlan) -> EditWriteDecision {
        match self {
            Self::AutoApprove => EditWriteDecision::Approve,
        }
    }
}

fn decide_edit_write(path: &std::path::Path, plan: &crate::tools::fs::core::EditPlan) -> EditWriteDecision {
    EditWritePolicy::AutoApprove.decide(path, plan)
}

fn parse_edit_mode(value: Option<&str>) -> Result<EditToolMode, BuiltinFsToolError> {
    match value.unwrap_or("apply") {
        "preview" => Ok(EditToolMode::Preview),
        "apply" => Ok(EditToolMode::Apply),
        other => Err(BuiltinFsToolError {
            kind: ToolErrorKind::Validation,
            message: format!("Invalid edit.mode '{other}': expected 'preview' or 'apply'"),
            details: None,
        }),
    }
}

fn make_edit_diagnostic(class: &str, message: impl Into<String>) -> JsonValue {
    serde_json::json!({
        "class": class,
        "message": message.into(),
    })
}

fn build_edit_preview_display(path: &str, plan: &crate::tools::fs::core::EditPlan) -> ToolDisplay {
    let diff = crate::tools::fs::diff::compute_edit_unified_diff(
        std::path::Path::new("file"),
        &plan.previous_content,
        &plan.new_content,
    );

    ToolDisplay {
        title: format!("edit {path}"),
        sections: vec![ToolDisplaySection {
            label: path.to_string(),
            language: "diff".to_string(),
            content: diff.text,
            stats: Some(ToolDisplayStats {
                files_changed: Some(diff.stats.files_changed),
                insertions: Some(diff.stats.insertions),
                deletions: Some(diff.stats.deletions),
                diff_truncated: Some(diff.truncated),
                omitted_files: Some(diff.omitted_files),
                omitted_hunks: Some(diff.omitted_hunks),
            }),
        }],
    }
}

fn attach_display_payload(response: &mut JsonValue, display: &ToolDisplay) {
    let sections = display
        .sections
        .iter()
        .map(|section| {
            let mut section_obj = serde_json::Map::new();
            section_obj.insert("label".to_string(), JsonValue::String(section.label.clone()));
            section_obj.insert(
                "language".to_string(),
                JsonValue::String(section.language.clone()),
            );
            section_obj.insert("content".to_string(), JsonValue::String(section.content.clone()));
            if let Some(stats) = &section.stats {
                let mut stats_obj = serde_json::Map::new();
                if let Some(files_changed) = stats.files_changed {
                    stats_obj.insert("files_changed".to_string(), JsonValue::from(files_changed));
                }
                if let Some(insertions) = stats.insertions {
                    stats_obj.insert("insertions".to_string(), JsonValue::from(insertions));
                }
                if let Some(deletions) = stats.deletions {
                    stats_obj.insert("deletions".to_string(), JsonValue::from(deletions));
                }
                if let Some(diff_truncated) = stats.diff_truncated {
                    stats_obj.insert("diff_truncated".to_string(), JsonValue::Bool(diff_truncated));
                }
                if let Some(omitted_files) = stats.omitted_files {
                    stats_obj.insert("omitted_files".to_string(), JsonValue::from(omitted_files));
                }
                if let Some(omitted_hunks) = stats.omitted_hunks {
                    stats_obj.insert("omitted_hunks".to_string(), JsonValue::from(omitted_hunks));
                }
                section_obj.insert("stats".to_string(), JsonValue::Object(stats_obj));
            }
            JsonValue::Object(section_obj)
        })
        .collect::<Vec<_>>();

    let mut display_obj = serde_json::Map::new();
    display_obj.insert("title".to_string(), JsonValue::String(display.title.clone()));
    display_obj.insert("sections".to_string(), JsonValue::Array(sections));

    if let Some(obj) = response.as_object_mut() {
        obj.insert("display".to_string(), JsonValue::Object(display_obj));
    }
}

fn resolve_edit_operation(args: &BuiltinEditArgs) -> Result<crate::tools::fs::core::EditOperation, BuiltinFsToolError> {
    if let Some(operation) = &args.operation {
        if let Some(operation_type) = operation.operation_type.as_deref()
            && operation_type != "search_replace"
        {
            return Err(BuiltinFsToolError {
                kind: ToolErrorKind::Validation,
                message: format!(
                    "Invalid edit.operation.type '{operation_type}': expected 'search_replace'"
                ),
                details: None,
            });
        }

        let match_mode = parse_edit_match_mode(operation.match_mode.as_deref())?;
        let occurrence = parse_edit_occurrence(operation.occurrence.as_deref())?;
        return Ok(crate::tools::fs::core::EditOperation {
            search: operation.search.clone(),
            replacement: operation.replacement.clone(),
            match_mode,
            occurrence,
        });
    }

    let search = args.search.clone().ok_or(BuiltinFsToolError {
        kind: ToolErrorKind::Validation,
        message: "Invalid edit arguments: missing field `search`".to_string(),
        details: None,
    })?;
    let replacement = args.replacement.clone().ok_or(BuiltinFsToolError {
        kind: ToolErrorKind::Validation,
        message: "Invalid edit arguments: missing field `replacement`".to_string(),
        details: None,
    })?;
    let match_mode = parse_edit_match_mode(args.match_mode.as_deref())?;
    let occurrence = parse_edit_occurrence(args.occurrence.as_deref())?;

    Ok(crate::tools::fs::core::EditOperation {
        search,
        replacement,
        match_mode,
        occurrence,
    })
}

fn map_edit_contract_error(error: &BuiltinFsToolError) -> &'static str {
    if let Some(class) = error
        .details
        .as_ref()
        .and_then(|details| details.get("diagnostic_class"))
        .and_then(serde_json::Value::as_str)
    {
        return match class {
            "validation" => "validation",
            "stale" => "stale",
            "permission" => "permission",
            "conflict" => "conflict",
            _ => "internal",
        };
    }

    match error.kind {
        ToolErrorKind::Validation => "validation",
        ToolErrorKind::Runtime => "internal",
        ToolErrorKind::Transport | ToolErrorKind::Timeout | ToolErrorKind::Unknown => "internal",
    }
}

#[cfg(test)]
type EditApplyPreviewHook = fn(&std::path::Path, &ToolDisplay);
#[cfg(test)]
type EditApplyDecisionHook = fn(&std::path::Path, &EditWriteDecision);

#[cfg(test)]
thread_local! {
    static EDIT_APPLY_POST_PLAN_HOOK: RefCell<Option<fn(&std::path::Path)>> = RefCell::new(None);
    static EDIT_APPLY_PREVIEW_HOOK: RefCell<Option<EditApplyPreviewHook>> = RefCell::new(None);
    static EDIT_APPLY_DECISION_HOOK: RefCell<Option<EditApplyDecisionHook>> = RefCell::new(None);
}

#[cfg(test)]
fn set_edit_apply_post_plan_hook(hook: Option<fn(&std::path::Path)>) {
    EDIT_APPLY_POST_PLAN_HOOK.with(|slot| {
        *slot.borrow_mut() = hook;
    });
}

#[cfg(test)]
fn set_edit_apply_preview_hook(hook: Option<EditApplyPreviewHook>) {
    EDIT_APPLY_PREVIEW_HOOK.with(|slot| {
        *slot.borrow_mut() = hook;
    });
}

#[cfg(test)]
fn set_edit_apply_decision_hook(hook: Option<EditApplyDecisionHook>) {
    EDIT_APPLY_DECISION_HOOK.with(|slot| {
        *slot.borrow_mut() = hook;
    });
}

#[cfg(test)]
fn run_edit_apply_post_plan_hook(path: &std::path::Path) {
    let hook = EDIT_APPLY_POST_PLAN_HOOK.with(|slot| *slot.borrow());
    if let Some(hook) = hook {
        hook(path);
    }
}

#[cfg(test)]
fn run_edit_apply_preview_hook(path: &std::path::Path, display: &ToolDisplay) {
    let hook = EDIT_APPLY_PREVIEW_HOOK.with(|slot| *slot.borrow());
    if let Some(hook) = hook {
        hook(path, display);
    }
}

#[cfg(not(test))]
fn run_edit_apply_preview_hook(_path: &std::path::Path, _display: &ToolDisplay) {}

#[cfg(test)]
fn run_edit_apply_decision_hook(path: &std::path::Path, decision: &EditWriteDecision) {
    let hook = EDIT_APPLY_DECISION_HOOK.with(|slot| *slot.borrow());
    if let Some(hook) = hook {
        hook(path, decision);
    }
}

#[cfg(not(test))]
fn run_edit_apply_decision_hook(_path: &std::path::Path, _decision: &EditWriteDecision) {}

#[cfg(not(test))]
fn run_edit_apply_post_plan_hook(_path: &std::path::Path) {}

fn build_edit_contract_response(
    path: &str,
    mode: EditToolMode,
    plan: crate::tools::fs::core::EditPlan,
    applied: bool,
) -> JsonValue {
    let diff = crate::tools::fs::diff::compute_edit_unified_diff(
        std::path::Path::new("file"),
        &plan.previous_content,
        &plan.new_content,
    );

    let mut diagnostics = Vec::new();
    if plan.conflict {
        diagnostics.push(make_edit_diagnostic(
            "stale",
            format!(
                "stale expected_version '{}' (current '{}')",
                plan.expected_version, plan.previous_version
            ),
        ));
    }

    serde_json::json!({
        "path": path,
        "mode": mode.as_str(),
        "proposal_id": serde_json::Value::Null,
        "applied": applied,
        "would_change": plan.would_change,
        "diff": diff.text,
        "stats": {
            "replacements": plan.replacements,
            "previous_bytes": plan.previous_bytes,
            "new_bytes": plan.new_bytes,
            "previous_lines": plan.previous_lines,
            "new_lines": plan.new_lines,
            "files_changed": diff.stats.files_changed,
            "insertions": diff.stats.insertions,
            "deletions": diff.stats.deletions,
            "diff_truncated": diff.truncated,
            "omitted_files": diff.omitted_files,
            "omitted_hunks": diff.omitted_hunks
        },
        "diagnostics": diagnostics,
        "changed": plan.would_change,
        "replacements": plan.replacements,
        "wrote": applied && plan.would_change,
        "noop": plan.noop,
        "conflict": plan.conflict,
        "expected_version": plan.expected_version,
        "previous_version": plan.previous_version,
        "new_version": plan.new_version,
    })
}

fn build_edit_contract_error_response(
    path: &str,
    mode: EditToolMode,
    class: &str,
    message: impl Into<String>,
) -> JsonValue {
    serde_json::json!({
        "path": path,
        "mode": mode.as_str(),
        "proposal_id": serde_json::Value::Null,
        "applied": false,
        "would_change": false,
        "diff": "",
        "stats": {
            "replacements": 0,
            "previous_bytes": 0,
            "new_bytes": 0,
            "previous_lines": 0,
            "new_lines": 0,
            "files_changed": 0,
            "insertions": 0,
            "deletions": 0,
            "diff_truncated": false,
            "omitted_files": 0,
            "omitted_hunks": 0
        },
        "diagnostics": [make_edit_diagnostic(class, message)],
    })
}

fn map_mutate_error(error: crate::tools::fs::core::MutateError) -> BuiltinFsToolError {
    use crate::tools::fs::core::MutateError;

    match error {
        MutateError::Io(io_error) => BuiltinFsToolError {
            kind: ToolErrorKind::Runtime,
            message: io_error.to_string(),
            details: Some(serde_json::json!({
                "io_kind": format!("{:?}", io_error.kind()),
                "diagnostic_class": if io_error.kind() == std::io::ErrorKind::PermissionDenied {
                    "permission"
                } else {
                    "internal"
                }
            })),
        },
        MutateError::Conflict(_) => BuiltinFsToolError {
            kind: ToolErrorKind::Validation,
            message: error.to_string(),
            details: Some(serde_json::json!({
                "diagnostic_class": "stale"
            })),
        },
        other => BuiltinFsToolError {
            kind: ToolErrorKind::Validation,
            message: other.to_string(),
            details: Some(serde_json::json!({
                "diagnostic_class": "validation"
            })),
        },
    }
}

fn resolve_builtin_fs_path_for_cwd(path: &str, cwd: &std::path::Path) -> std::path::PathBuf {
    let raw = std::path::Path::new(path);
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    }
}

fn resolve_builtin_fs_path(
    path: &str,
    engine: &EngineInterface,
) -> Result<std::path::PathBuf, BuiltinFsToolError> {
    let cwd = engine.get_current_dir().map_err(|e| BuiltinFsToolError {
        kind: ToolErrorKind::Runtime,
        message: format!("Unable to resolve current working directory: {e}"),
        details: None,
    })?;
    Ok(resolve_builtin_fs_path_for_cwd(path, std::path::Path::new(&cwd)))
}

fn dispatch_builtin_fs_tool(
    tool_name: &str,
    arguments: &JsonValue,
    cwd: &std::path::Path,
) -> Result<Option<JsonValue>, BuiltinFsToolError> {
    use crate::tools::fs::core::{
        PatchOp, PatchRange, ReadRequest, apply_line_range_patch_batch,
        apply_search_replace_edit, read_file,
    };

    match tool_name {
        "read" => {
            let args: BuiltinReadArgs =
                serde_json::from_value(arguments.clone()).map_err(|e| BuiltinFsToolError {
                    kind: ToolErrorKind::Validation,
                    message: format!("Invalid read arguments: {e}"),
                    details: None,
                })?;

            let resolved_path = resolve_builtin_fs_path_for_cwd(&args.path, cwd);
            let response = read_file(
                &resolved_path,
                ReadRequest {
                    offset: args.offset,
                    limit: args.limit,
                },
            )
            .map_err(|e| BuiltinFsToolError {
                kind: ToolErrorKind::Runtime,
                message: format!("read failed: {e}"),
                details: None,
            })?;

            Ok(Some(serde_json::json!({
                "path": args.path,
                "content": response.content,
                "total_lines": response.total_lines,
                "offset": response.offset,
                "limit": response.limit,
                "version": response.version,
            })))
        }
        "edit" => {
            let args: BuiltinEditArgs =
                serde_json::from_value(arguments.clone()).map_err(|e| BuiltinFsToolError {
                    kind: ToolErrorKind::Validation,
                    message: format!("Invalid edit arguments: {e}"),
                    details: None,
                })?;

            let resolved_path = resolve_builtin_fs_path_for_cwd(&args.path, cwd);
            let mode = match parse_edit_mode(args.mode.as_deref()) {
                Ok(mode) => mode,
                Err(err) => {
                    return Ok(Some(build_edit_contract_error_response(
                        &args.path,
                        EditToolMode::Apply,
                        map_edit_contract_error(&err),
                        err.message,
                    )));
                }
            };

            let operation = match resolve_edit_operation(&args) {
                Ok(operation) => operation,
                Err(err) => {
                    return Ok(Some(build_edit_contract_error_response(
                        &args.path,
                        mode,
                        map_edit_contract_error(&err),
                        err.message,
                    )));
                }
            };

            let plan = match crate::tools::fs::core::plan_search_replace_edit(
                &resolved_path,
                args.expected_version.as_deref(),
                &operation,
            ) {
                Ok(plan) => plan,
                Err(err) => {
                    let mapped = map_mutate_error(err);
                    return Ok(Some(build_edit_contract_error_response(
                        &args.path,
                        mode,
                        map_edit_contract_error(&mapped),
                        mapped.message,
                    )));
                }
            };

            match mode {
                EditToolMode::Preview => {
                    Ok(Some(build_edit_contract_response(&args.path, mode, plan, false)))
                }
                EditToolMode::Apply => {
                    let preview_display = build_edit_preview_display(&args.path, &plan);
                    run_edit_apply_preview_hook(&resolved_path, &preview_display);

                    let decision = decide_edit_write(&resolved_path, &plan);
                    run_edit_apply_decision_hook(&resolved_path, &decision);

                    if plan.conflict || !plan.would_change {
                        let mut response = build_edit_contract_response(&args.path, mode, plan, false);
                        attach_display_payload(&mut response, &preview_display);
                        return Ok(Some(response));
                    }

                    if let EditWriteDecision::Deny { message } = decision {
                        let mut response = build_edit_contract_response(&args.path, mode, plan, false);
                        if let Some(obj) = response.as_object_mut()
                            && let Some(diagnostics) = obj.get_mut("diagnostics").and_then(JsonValue::as_array_mut)
                        {
                            diagnostics.push(make_edit_diagnostic("permission", message));
                        }
                        attach_display_payload(&mut response, &preview_display);
                        return Ok(Some(response));
                    }

                    run_edit_apply_post_plan_hook(&resolved_path);

                    let summary = match apply_search_replace_edit(
                        &resolved_path,
                        args.expected_version.as_deref(),
                        &operation,
                    )
                    {
                        Ok(summary) => summary,
                        Err(crate::tools::fs::core::MutateError::Conflict(_)) => {
                            let refreshed_plan = match crate::tools::fs::core::plan_search_replace_edit(
                                &resolved_path,
                                args.expected_version.as_deref(),
                                &operation,
                            ) {
                                Ok(refreshed_plan) => refreshed_plan,
                                Err(err) => {
                                    let mapped = map_mutate_error(err);
                                    let mut response = build_edit_contract_error_response(
                                        &args.path,
                                        mode,
                                        map_edit_contract_error(&mapped),
                                        mapped.message,
                                    );
                                    attach_display_payload(&mut response, &preview_display);
                                    return Ok(Some(response));
                                }
                            };
                            let mut response = build_edit_contract_response(
                                &args.path,
                                mode,
                                refreshed_plan,
                                false,
                            );
                            attach_display_payload(&mut response, &preview_display);
                            return Ok(Some(response));
                        }
                        Err(err) => {
                            let mapped = map_mutate_error(err);
                            let mut response = build_edit_contract_error_response(
                                &args.path,
                                mode,
                                map_edit_contract_error(&mapped),
                                mapped.message,
                            );
                            attach_display_payload(&mut response, &preview_display);
                            return Ok(Some(response));
                        }
                    };

                    if summary.conflict {
                        let refreshed_plan = match crate::tools::fs::core::plan_search_replace_edit(
                            &resolved_path,
                            args.expected_version.as_deref(),
                            &operation,
                        ) {
                            Ok(refreshed_plan) => refreshed_plan,
                            Err(err) => {
                                let mapped = map_mutate_error(err);
                                let mut response = build_edit_contract_error_response(
                                    &args.path,
                                    mode,
                                    map_edit_contract_error(&mapped),
                                    mapped.message,
                                );
                                attach_display_payload(&mut response, &preview_display);
                                return Ok(Some(response));
                            }
                        };
                        let mut response = build_edit_contract_response(
                            &args.path,
                            mode,
                            refreshed_plan,
                            false,
                        );
                        attach_display_payload(&mut response, &preview_display);
                        return Ok(Some(response));
                    }

                    let mut response = build_edit_contract_response(&args.path, mode, plan, summary.wrote);
                    if let Some(obj) = response.as_object_mut() {
                        obj.insert("wrote".to_string(), JsonValue::Bool(summary.wrote));
                        obj.insert("changed".to_string(), JsonValue::Bool(summary.changed));
                        obj.insert("noop".to_string(), JsonValue::Bool(summary.noop));
                        obj.insert("conflict".to_string(), JsonValue::Bool(summary.conflict));
                        obj.insert(
                            "expected_version".to_string(),
                            JsonValue::String(summary.expected_version),
                        );
                        obj.insert(
                            "previous_version".to_string(),
                            JsonValue::String(summary.previous_version),
                        );
                        obj.insert("new_version".to_string(), JsonValue::String(summary.new_version));
                    }
                    attach_display_payload(&mut response, &preview_display);
                    Ok(Some(response))
                }
            }
        }
        "patch" => {
            let args: BuiltinPatchArgs =
                serde_json::from_value(arguments.clone()).map_err(|e| BuiltinFsToolError {
                    kind: ToolErrorKind::Validation,
                    message: format!("Invalid patch arguments: {e}"),
                    details: None,
                })?;

            let resolved_path = resolve_builtin_fs_path_for_cwd(&args.path, cwd);
            let operations = args
                .operations
                .into_iter()
                .map(|op| PatchOp {
                    range: PatchRange::new(op.range.start, op.range.end),
                    replacement: op.replacement,
                })
                .collect::<Vec<_>>();

            let summary = apply_line_range_patch_batch(
                &resolved_path,
                args.expected_version.as_deref(),
                operations,
            )
            .map_err(map_mutate_error)?;

            Ok(Some(serde_json::json!({
                "path": args.path,
                "operation_count": summary.operation_count,
                "applied_ranges": summary
                    .applied_ranges
                    .iter()
                    .map(|range| serde_json::json!({"start": range.start, "end": range.end}))
                    .collect::<Vec<_>>(),
                "wrote": summary.wrote,
                "changed": summary.changed,
                "noop": summary.noop,
                "conflict": summary.conflict,
                "expected_version": summary.expected_version,
                "previous_version": summary.previous_version,
                "new_version": summary.new_version,
                "previous_lines": summary.previous_lines,
                "new_lines": summary.new_lines,
            })))
        }
        "skill" => {
            let args: BuiltinSkillArgs =
                serde_json::from_value(arguments.clone()).map_err(|e| BuiltinFsToolError {
                    kind: ToolErrorKind::Validation,
                    message: format!("Invalid skill arguments: {e}"),
                    details: None,
                })?;

            let resolved = crate::agent::protocol::skills::resolve_explicit_skill_request_for_cwd(
                cwd,
                &args.name,
            )
            .map_err(|e| BuiltinFsToolError {
                kind: ToolErrorKind::Validation,
                message: format!("skill resolution failed: {e}"),
                details: None,
            })?;

            let payload = match resolved {
                Some(resolved) => serde_json::json!({
                    "name": resolved.name,
                    "source": resolved.source.label(),
                    "path": resolved.path,
                    "content": resolved.content,
                }),
                None => serde_json::json!({
                    "name": args.name,
                    "found": false,
                }),
            };

            Ok(Some(payload))
        }
        _ => Ok(None),
    }
}

/// Convert a serde_json::Value to nu_protocol::Value.
///
/// Recursively converts JSON values to their Nushell equivalents.
///
/// # Arguments
/// * `json` - The JSON value to convert
/// * `span` - The span for error reporting and value creation
///
/// # Returns
/// A Nushell Value, or ShellError if conversion fails
#[allow(clippy::result_large_err)]
pub fn json_to_nu_value(json: &JsonValue, span: Span) -> Result<Value, GenericError> {
    match json {
        JsonValue::Null => Ok(Value::nothing(span)),
        JsonValue::Bool(b) => Ok(Value::bool(*b, span)),
        JsonValue::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(Value::int(i, span))
            } else if let Some(f) = n.as_f64() {
                Ok(Value::float(f, span))
            } else {
                Err(GenericError::new(
                    "Invalid JSON number",
                    "Could not convert number",
                    span,
                ))
            }
        }
        JsonValue::String(s) => Ok(Value::string(s.clone(), span)),
        JsonValue::Array(arr) => {
            let values: Result<Vec<Value>, GenericError> = arr
                .iter()
                .map(|item| json_to_nu_value(item, span))
                .collect();
            Ok(Value::list(values?, span))
        }
        JsonValue::Object(obj) => {
            let mut record = nu_protocol::record!();
            for (key, value) in obj {
                record.insert(key.clone(), json_to_nu_value(value, span)?);
            }
            Ok(Value::record(record, span))
        }
    }
}

/// Convert a nu_protocol::Value to serde_json::Value.
///
/// Recursively converts Nushell values to their JSON equivalents.
///
/// # Arguments
/// * `value` - The Nushell value to convert
///
/// # Returns
/// A JSON value, or ShellError if conversion fails
#[allow(clippy::result_large_err)]
pub fn nu_value_to_json(value: &Value) -> Result<JsonValue, GenericError> {
    match value {
        Value::Nothing { .. } => Ok(JsonValue::Null),
        Value::Bool { val, .. } => Ok(JsonValue::Bool(*val)),
        Value::Int { val, .. } => Ok(JsonValue::Number((*val).into())),
        Value::Float { val, .. } => serde_json::Number::from_f64(*val)
            .map(JsonValue::Number)
            .ok_or_else(|| {
                GenericError::new(
                    "Invalid float value",
                    "Cannot convert float to JSON",
                    value.span(),
                )
            }),
        Value::String { val, .. } => Ok(JsonValue::String(val.clone())),
        Value::List { vals, .. } => {
            let json_values: Result<Vec<JsonValue>, GenericError> =
                vals.iter().map(nu_value_to_json).collect();
            Ok(JsonValue::Array(json_values?))
        }
        Value::Record { val, .. } => {
            let mut map = serde_json::Map::new();
            for (key, value) in val.iter() {
                map.insert(key.clone(), nu_value_to_json(value)?);
            }
            Ok(JsonValue::Object(map))
        }
        _ => Err(GenericError::new(
            "Unsupported value type",
            format!("Cannot convert {:?} to JSON", value),
            value.span(),
        )),
    }
}

/// Result of executing a single tool call.
///
/// Contains the tool call ID and the serialized JSON result.
#[derive(Debug, Clone)]
pub struct ToolCallResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub arguments: String,
    pub source: ToolSource,
    pub content: String,
    pub display: Option<ToolDisplay>,
    pub failure: Option<ToolFailureOutcome>,
}

/// Handle multiple tool calls from LLM response.
///
/// Executes each tool call sequentially and returns a list of results.
///
/// # Arguments
/// * `tool_calls` - List of AssistantContent items that may contain tool calls
/// * `closure_registry` - Registry to look up tool closures by name
/// * `tool_executor` - Executor for running the closures
/// * `engine` - Engine interface for extracting closure parameter names
/// * `span` - Span for error reporting
///
/// # Returns
/// Vector of ToolCallResult, one for each successful tool call execution
pub async fn handle_tool_calls(
    tool_calls: Vec<AssistantContent>,
    closure_registry: &ClosureRegistry,
    mcp_registry: &McpToolRegistry,
    mcp_tool_server: Option<&rig::tool::server::ToolServerHandle>,
    tool_executor: &ToolExecutor,
    engine: &EngineInterface,
    span: Span,
) -> Vec<ToolCallResult> {
    let mut results = Vec::new();

    for content in tool_calls {
        // Only process ToolCall variants
        if let AssistantContent::ToolCall(tool_call) = content {
            let result = handle_single_tool_call(
                tool_call,
                closure_registry,
                mcp_registry,
                mcp_tool_server,
                tool_executor,
                engine,
                span,
            )
            .await;

            results.push(result);
        }
    }

    results
}

fn classify_validation_error_message(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("invalid")
        || lower.contains("must be")
        || lower.contains("expected")
        || lower.contains("missing")
        || lower.contains("parse")
}

fn build_failure_result(
    tool_call: &ToolCall,
    source: ToolSource,
    error_kind: ToolErrorKind,
    message: String,
    details: Option<JsonValue>,
) -> ToolCallResult {
    let serialized_arguments =
        serde_json::to_string(&tool_call.function.arguments).unwrap_or_else(|_| "{}".to_string());

    let failure = ToolFailureOutcome {
        tool_name: tool_call.function.name.clone(),
        tool_call_id: tool_call.id.clone(),
        source: source.clone(),
        error_kind,
        message,
        details,
    };

    ToolCallResult {
        tool_call_id: tool_call.id.clone(),
        tool_name: tool_call.function.name.clone(),
        arguments: serialized_arguments,
        source,
        content: failure.to_json_string(),
        display: None,
        failure: Some(failure),
    }
}

fn parse_display_stats(stats: Option<&JsonValue>) -> Option<ToolDisplayStats> {
    let stats = stats?.as_object()?;
    Some(ToolDisplayStats {
        files_changed: stats
            .get("files_changed")
            .and_then(JsonValue::as_u64)
            .map(|v| v as usize),
        insertions: stats
            .get("insertions")
            .and_then(JsonValue::as_u64)
            .map(|v| v as usize),
        deletions: stats
            .get("deletions")
            .and_then(JsonValue::as_u64)
            .map(|v| v as usize),
        diff_truncated: stats.get("diff_truncated").and_then(JsonValue::as_bool),
        omitted_files: stats
            .get("omitted_files")
            .and_then(JsonValue::as_u64)
            .map(|v| v as usize),
        omitted_hunks: stats
            .get("omitted_hunks")
            .and_then(JsonValue::as_u64)
            .map(|v| v as usize),
    })
}

fn tool_display_from_minimal_object(display: &JsonValue) -> Option<ToolDisplay> {
    let display = display.as_object()?;
    if display.contains_key("kind") {
        return None;
    }
    let title = display.get("title")?.as_str()?.to_string();
    let sections = display.get("sections")?.as_array()?;
    let mut parsed_sections = Vec::with_capacity(sections.len());
    for section in sections {
        let section = section.as_object()?;
        if section.contains_key("kind") {
            return None;
        }
        parsed_sections.push(ToolDisplaySection {
            label: section.get("label")?.as_str()?.to_string(),
            language: section.get("language")?.as_str()?.to_string(),
            content: section.get("content")?.as_str()?.to_string(),
            stats: parse_display_stats(section.get("stats")),
        });
    }
    if parsed_sections.is_empty() {
        return None;
    }
    Some(ToolDisplay {
        title,
        sections: parsed_sections,
    })
}

fn build_direct_tool_display(tool_name: &str, payload: &JsonValue) -> Option<ToolDisplay> {
    if let Some(explicit_display) = payload.get("display")
        && let Some(display) = tool_display_from_minimal_object(explicit_display)
    {
        return Some(display);
    }

    if tool_name != "edit" {
        return None;
    }

    let path = payload.get("path")?.as_str()?;
    let diff = payload
        .get("diff")
        .and_then(JsonValue::as_str)
        .unwrap_or_default()
        .to_string();

    Some(ToolDisplay {
        title: format!("edit {path}"),
        sections: vec![ToolDisplaySection {
            label: path.to_string(),
            language: "diff".to_string(),
            content: diff,
            stats: parse_display_stats(payload.get("stats")),
        }],
    })
}

/// Handle a single tool call.
///
/// Looks up the tool closure, parses arguments, executes it, and returns the result.
/// Arguments are extracted by parameter name and passed to the closure in the correct order.
///
/// # Arguments
/// * `tool_call` - The ToolCall from rig-rs containing id, function (with name and arguments)
/// * `closure_registry` - Registry to look up tool closures by name
/// * `tool_executor` - Executor for running the closures
/// * `engine` - Engine interface for extracting closure parameter names
/// * `span` - Span for error reporting
///
/// # Returns
/// ToolCallResult with the tool call ID and JSON-serialized result
async fn handle_single_tool_call(
    tool_call: ToolCall,
    closure_registry: &ClosureRegistry,
    mcp_registry: &McpToolRegistry,
    mcp_tool_server: Option<&rig::tool::server::ToolServerHandle>,
    tool_executor: &ToolExecutor,
    engine: &EngineInterface,
    span: Span,
) -> ToolCallResult {
    // Look up closure by function name
    let serialized_arguments =
        serde_json::to_string(&tool_call.function.arguments).unwrap_or_else(|_| "{}".to_string());

    let source = if let Some(source) =
        classify_tool_source(&tool_call.function.name, closure_registry, mcp_registry)
    {
        source
    } else {
        return build_failure_result(
            &tool_call,
            ToolSource::Unknown,
            ToolErrorKind::Unknown,
            format!("Tool '{}' not found", tool_call.function.name),
            None,
        );
    };

    if source == ToolSource::Mcp {
        let Some(server) = mcp_tool_server else {
            return build_failure_result(
                &tool_call,
                ToolSource::Mcp,
                ToolErrorKind::Transport,
                "MCP runtime unavailable: MCP tool server handle is not initialized".to_string(),
                None,
            );
        };

        let raw_tool_name = if let Some(name) =
            resolve_mcp_invocation_name(mcp_registry, &tool_call.function.name)
        {
            name
        } else {
            return build_failure_result(
                &tool_call,
                ToolSource::Mcp,
                ToolErrorKind::Runtime,
                format!(
                    "MCP tool '{}' is registered but missing raw-name mapping",
                    tool_call.function.name
                ),
                None,
            );
        };

        let content = match server.call_tool(raw_tool_name, &serialized_arguments).await {
            Ok(content) => content,
            Err(e) => {
                return build_failure_result(
                    &tool_call,
                    ToolSource::Mcp,
                    ToolErrorKind::Transport,
                    format!("MCP tool execution failed: {e}"),
                    None,
                );
            }
        };

        return ToolCallResult {
            tool_call_id: tool_call.id,
            tool_name: tool_call.function.name,
            arguments: serialized_arguments,
            source,
            content,
            display: None,
            failure: None,
        };
    }

    let builtin_cwd = match resolve_builtin_fs_path(".", engine) {
        Ok(path) => path,
        Err(err) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                err.kind,
                format!("Tool execution failed: {}", err.message),
                err.details,
            );
        }
    };

    match dispatch_builtin_fs_tool(&tool_call.function.name, &tool_call.function.arguments, &builtin_cwd) {
        Ok(Some(payload)) => {
            let display = build_direct_tool_display(&tool_call.function.name, &payload);
            let content = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
            return ToolCallResult {
                tool_call_id: tool_call.id,
                tool_name: tool_call.function.name,
                arguments: serialized_arguments,
                source,
                content,
                display,
                failure: None,
            };
        }
        Ok(None) => {}
        Err(err) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                err.kind,
                format!("Tool execution failed: {}", err.message),
                err.details,
            );
        }
    }

    let Some(closure) = closure_registry.get(&tool_call.function.name) else {
        return build_failure_result(
            &tool_call,
            ToolSource::Closure,
            ToolErrorKind::Unknown,
            format!("Tool '{}' not found", tool_call.function.name),
            None,
        );
    };

    // Parse arguments from JSON Value
    let args_json = match json_to_nu_value(&tool_call.function.arguments, span) {
        Ok(v) => v,
        Err(e) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                ToolErrorKind::Validation,
                format!("Invalid tool arguments: {e}"),
                None,
            );
        }
    };

    // Extract positional arguments by matching parameter names
    let positional_args = if let Value::Record { val, .. } = &args_json {
        // Get parameter names from closure source
        use crate::tools::closure::extract_parameter_names;
        let param_names = extract_parameter_names(closure, engine);

        // Extract values in parameter order
        param_names
            .iter()
            .map(|name| {
                val.get(name)
                    .cloned()
                    .unwrap_or_else(|| Value::nothing(span))
            })
            .collect()
    } else {
        // Not a record - pass as single argument (fallback for compatibility)
        vec![args_json]
    };

    // Execute closure via ToolExecutor (closure is already Spanned)
    let result = tool_executor
        .invoke_closure(closure, positional_args, span)
        .await;

    let result = match result {
        Ok(result) => result,
        Err(ToolError::Timeout { tool_name, timeout }) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                ToolErrorKind::Timeout,
                format!("Tool '{}' timed out after {:?}", tool_name, timeout),
                Some(serde_json::json!({ "timeout_ms": timeout.as_millis() })),
            );
        }
        Err(ToolError::Execution(err)) => {
            let msg = err.to_string();
            let kind = if classify_validation_error_message(&msg) {
                ToolErrorKind::Validation
            } else {
                ToolErrorKind::Runtime
            };

            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                kind,
                format!("Tool execution failed: {msg}"),
                None,
            );
        }
        Err(ToolError::Audit(err)) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                ToolErrorKind::Runtime,
                format!("Tool audit failed: {err}"),
                None,
            );
        }
    };

    // Convert result back to JSON string
    let result_json = match nu_value_to_json(&result) {
        Ok(v) => v,
        Err(e) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                ToolErrorKind::Runtime,
                format!("Result conversion failed: {e}"),
                None,
            );
        }
    };
    let content = match serde_json::to_string(&result_json) {
        Ok(content) => content,
        Err(e) => {
            return build_failure_result(
                &tool_call,
                ToolSource::Closure,
                ToolErrorKind::Runtime,
                format!("Result serialization failed: {e}"),
                None,
            );
        }
    };

    ToolCallResult {
        tool_call_id: tool_call.id,
        tool_name: tool_call.function.name,
        arguments: serialized_arguments,
        source,
        content,
        display: None,
        failure: None,
    }
}

#[cfg(test)]
mod tests;
