use std::path::Path;

use serde_json::Value as JsonValue;

use super::{
    ToolErrorKind, ToolHandlerError,
    builtin_tool::{BuiltinTool, Previewable},
};
use crate::bus::Bus;
use crate::protocol::event::{ToolDisplay, ToolDisplaySection};
use crate::protocol::tool_args::{CallLine, parse_json_string_field};
use crate::tools::fs::core::apply_search_replace_edit;
use crate::transcript::ir::ContentKind;

#[derive(Debug, serde::Deserialize)]
pub(super) struct EditArgs {
    pub(super) path: String,
    #[serde(default)]
    pub(super) expected_version: Option<String>,
    #[serde(default)]
    pub(super) mode: Option<String>,
    pub(super) operation: EditOperationArgs,
}

#[derive(Debug, serde::Deserialize, Clone)]
pub(super) struct EditOperationArgs {
    #[serde(default)]
    #[serde(rename = "type")]
    operation_type: Option<String>,
    #[serde(default)]
    search: Option<String>,
    #[serde(default)]
    replacement: Option<String>,
    #[serde(default)]
    match_mode: Option<String>,
    #[serde(default)]
    occurrence: Option<String>,
    #[serde(default)]
    content: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) enum ResolvedEditOperation {
    SearchReplace(crate::tools::fs::core::EditOperation),
    Create { content: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditToolMode {
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

fn parse_edit_match_mode(
    value: Option<&str>,
) -> Result<crate::tools::fs::core::EditMatchMode, ToolHandlerError> {
    match value.unwrap_or("literal") {
        "literal" => Ok(crate::tools::fs::core::EditMatchMode::Literal),
        "regex" => Ok(crate::tools::fs::core::EditMatchMode::Regex),
        other => Err(ToolHandlerError {
            kind: ToolErrorKind::Validation,
            message: format!("Invalid edit.match_mode '{other}': expected 'literal' or 'regex'"),
            details: None,
        }),
    }
}

fn parse_edit_occurrence(
    value: Option<&str>,
) -> Result<crate::tools::fs::core::EditOccurrence, ToolHandlerError> {
    match value.unwrap_or("first") {
        "first" => Ok(crate::tools::fs::core::EditOccurrence::First),
        "all" => Ok(crate::tools::fs::core::EditOccurrence::All),
        other => Err(ToolHandlerError {
            kind: ToolErrorKind::Validation,
            message: format!("Invalid edit.occurrence '{other}': expected 'first' or 'all'"),
            details: None,
        }),
    }
}

pub fn parse_edit_mode(value: Option<&str>) -> Result<EditToolMode, ToolHandlerError> {
    match value.unwrap_or("apply") {
        "preview" => Ok(EditToolMode::Preview),
        "apply" => Ok(EditToolMode::Apply),
        other => Err(ToolHandlerError {
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

// region:    --- Support

/// Embed a tool display into a tool result JSON under the `display` key, so
/// the TUI/TTY layers can project it without re-deriving it from the raw
/// result payload.
fn embed_display_payload(response: &mut JsonValue, display: &ToolDisplay) {
    let sections = display
        .sections
        .iter()
        .map(|section| {
            let mut section_obj = serde_json::Map::new();
            section_obj.insert(
                "label".to_string(),
                JsonValue::String(section.label.clone()),
            );
            section_obj.insert(
                "language".to_string(),
                JsonValue::String(section.kind.language().to_string()),
            );
            section_obj.insert(
                "content".to_string(),
                JsonValue::String(section.content.clone()),
            );
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
                    stats_obj.insert(
                        "diff_truncated".to_string(),
                        JsonValue::Bool(diff_truncated),
                    );
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
    display_obj.insert(
        "title".to_string(),
        JsonValue::String(display.title.clone()),
    );
    display_obj.insert("sections".to_string(), JsonValue::Array(sections));

    if let Some(obj) = response.as_object_mut() {
        obj.insert("display".to_string(), JsonValue::Object(display_obj));
    }
}

// endregion: --- Support

/// Build the edit diff preview display from an edit plan. Shared by the
/// permission-gate preview (`Previewable::preview`) and the tool response
/// payload.
pub(super) fn edit_preview_display(
    path: &str,
    plan: &crate::tools::fs::core::EditPlan,
) -> ToolDisplay {
    let diff = crate::tools::fs::diff::compute_edit_unified_diff(
        std::path::Path::new("file"),
        &plan.previous_content,
        &plan.new_content,
    );

    ToolDisplay {
        title: format!("edit {path}"),
        sections: vec![ToolDisplaySection {
            label: path.to_string(),
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: diff.text,
            stats: Some(crate::protocol::event::ToolDisplayStats {
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

pub(super) fn resolve_edit_operation(
    args: &EditArgs,
) -> Result<ResolvedEditOperation, ToolHandlerError> {
    let operation = &args.operation;
    let op_type = operation
        .operation_type
        .as_deref()
        .unwrap_or("search_replace");

    match op_type {
        "create" => {
            let content = operation.content.clone().ok_or(ToolHandlerError {
                kind: ToolErrorKind::Validation,
                message:
                    "Invalid edit arguments: missing field `operation.content` for create operation"
                        .to_string(),
                details: None,
            })?;
            Ok(ResolvedEditOperation::Create { content })
        }
        "search_replace" => {
            let search = operation.search.clone().ok_or(ToolHandlerError {
                kind: ToolErrorKind::Validation,
                message: "Invalid edit arguments: missing field `operation.search` for search_replace operation".to_string(),
                details: None,
            })?;
            let replacement = operation.replacement.clone().ok_or(ToolHandlerError {
                kind: ToolErrorKind::Validation,
                message: "Invalid edit arguments: missing field `operation.replacement` for search_replace operation".to_string(),
                details: None,
            })?;
            let match_mode = parse_edit_match_mode(operation.match_mode.as_deref())?;
            let occurrence = parse_edit_occurrence(operation.occurrence.as_deref())?;
            Ok(ResolvedEditOperation::SearchReplace(
                crate::tools::fs::core::EditOperation {
                    search,
                    replacement,
                    match_mode,
                    occurrence,
                },
            ))
        }
        other => Err(ToolHandlerError {
            kind: ToolErrorKind::Validation,
            message: format!(
                "Invalid edit.operation.type '{other}': expected 'search_replace' or 'create'"
            ),
            details: None,
        }),
    }
}

fn map_edit_contract_error(error: &ToolHandlerError) -> &'static str {
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
        ToolErrorKind::Authorization => "internal",
        ToolErrorKind::Runtime => "internal",
        ToolErrorKind::Transport | ToolErrorKind::Timeout | ToolErrorKind::Unknown => "internal",
    }
}

fn build_edit_contract_response(
    path: &str,
    mode: EditToolMode,
    plan: crate::tools::fs::core::EditPlan,
    applied: bool,
    summary: Option<&crate::tools::fs::core::EditSummary>,
) -> JsonValue {
    let diff = crate::tools::fs::diff::compute_edit_unified_diff(
        std::path::Path::new("file"),
        &plan.previous_content,
        &plan.new_content,
    );

    let (wrote, changed, noop, conflict, expected_version, previous_version, new_version) =
        match summary {
            Some(s) => (
                s.wrote,
                s.changed,
                s.noop,
                s.conflict,
                s.expected_version.clone(),
                s.previous_version.clone(),
                s.new_version.clone(),
            ),
            None => (
                applied && plan.would_change,
                plan.would_change,
                plan.noop,
                plan.conflict,
                plan.expected_version,
                plan.previous_version,
                plan.new_version,
            ),
        };

    let mut diagnostics = Vec::new();
    if conflict {
        diagnostics.push(make_edit_diagnostic(
            "stale",
            format!(
                "stale expected_version '{}' (current '{}')",
                expected_version, previous_version
            ),
        ));
    }

    serde_json::json!({
        "path": path,
        "mode": mode.as_str(),
        "proposal_id": serde_json::Value::Null,
        "applied": applied,
        "would_change": changed,
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
        "changed": changed,
        "replacements": plan.replacements,
        "wrote": wrote,
        "noop": noop,
        "conflict": conflict,
        "expected_version": expected_version,
        "previous_version": previous_version,
        "new_version": new_version,
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

pub(crate) fn map_mutate_error(error: crate::tools::fs::core::MutateError) -> ToolHandlerError {
    use crate::tools::fs::core::MutateError;

    match error {
        MutateError::Io(io_error) => ToolHandlerError {
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
        MutateError::Conflict(_) => ToolHandlerError {
            kind: ToolErrorKind::Validation,
            message: error.to_string(),
            details: Some(serde_json::json!({
                "diagnostic_class": "stale"
            })),
        },
        other => ToolHandlerError {
            kind: ToolErrorKind::Validation,
            message: other.to_string(),
            details: Some(serde_json::json!({
                "diagnostic_class": "validation"
            })),
        },
    }
}

pub struct EditTool;

impl BuiltinTool for EditTool {
    const NAME: &'static str = "edit";

    fn call_line_render(arguments: &str) -> CallLine {
        let Some(path) = parse_json_string_field(arguments, "path") else {
            return CallLine::from_json_summary(arguments);
        };
        CallLine {
            summary: format!("→ {path} (diff)"),
        }
    }

    async fn execute(
        args: &JsonValue,
        cwd: &Path,
        _bus: &Bus,
    ) -> Result<JsonValue, ToolHandlerError> {
        let edit_args: EditArgs =
            serde_json::from_value(args.clone()).map_err(|e| ToolHandlerError {
                kind: ToolErrorKind::Validation,
                message: format!("Invalid edit arguments: {e}"),
                details: None,
            })?;

        let resolved_path = super::resolve_fs_path_for_cwd(&edit_args.path, cwd);
        let mode = match parse_edit_mode(edit_args.mode.as_deref()) {
            Ok(mode) => mode,
            Err(err) => {
                return Ok(build_edit_contract_error_response(
                    &edit_args.path,
                    EditToolMode::Apply,
                    map_edit_contract_error(&err),
                    err.message,
                ));
            }
        };

        let operation = match resolve_edit_operation(&edit_args) {
            Ok(operation) => operation,
            Err(err) => {
                return Ok(build_edit_contract_error_response(
                    &edit_args.path,
                    mode,
                    map_edit_contract_error(&err),
                    err.message,
                ));
            }
        };

        match operation {
            ResolvedEditOperation::SearchReplace(sr_op) => {
                let plan = match crate::tools::fs::core::plan_search_replace_edit(
                    &resolved_path,
                    edit_args.expected_version.as_deref(),
                    &sr_op,
                ) {
                    Ok(plan) => plan,
                    Err(err) => {
                        let mapped = map_mutate_error(err);
                        return Ok(build_edit_contract_error_response(
                            &edit_args.path,
                            mode,
                            map_edit_contract_error(&mapped),
                            mapped.message,
                        ));
                    }
                };

                match mode {
                    EditToolMode::Preview => Ok(build_edit_contract_response(
                        &edit_args.path,
                        mode,
                        plan,
                        false,
                        None,
                    )),
                    EditToolMode::Apply => {
                        let preview_display = super::pre_authorize::pre_authorize_fs_tool(
                            Some(super::builtin_kinds::BuiltinKind::Edit),
                            args,
                            cwd,
                        )
                        .and_then(|output| output.display)
                        .unwrap_or_else(|| edit_preview_display(&edit_args.path, &plan));

                        if plan.conflict || !plan.would_change {
                            let mut response = build_edit_contract_response(
                                &edit_args.path,
                                mode,
                                plan,
                                false,
                                None,
                            );
                            embed_display_payload(&mut response, &preview_display);
                            return Ok(response);
                        }

                        let summary = match apply_search_replace_edit(
                            &resolved_path,
                            edit_args.expected_version.as_deref(),
                            &sr_op,
                        ) {
                            Ok(summary) => summary,
                            Err(crate::tools::fs::core::MutateError::Conflict(_)) => {
                                let refreshed_plan =
                                    match crate::tools::fs::core::plan_search_replace_edit(
                                        &resolved_path,
                                        edit_args.expected_version.as_deref(),
                                        &sr_op,
                                    ) {
                                        Ok(refreshed_plan) => refreshed_plan,
                                        Err(err) => {
                                            let mapped = map_mutate_error(err);
                                            let mut response = build_edit_contract_error_response(
                                                &edit_args.path,
                                                mode,
                                                map_edit_contract_error(&mapped),
                                                mapped.message,
                                            );
                                            embed_display_payload(&mut response, &preview_display);
                                            return Ok(response);
                                        }
                                    };
                                let mut response = build_edit_contract_response(
                                    &edit_args.path,
                                    mode,
                                    refreshed_plan,
                                    false,
                                    None,
                                );
                                embed_display_payload(&mut response, &preview_display);
                                return Ok(response);
                            }
                            Err(err) => {
                                let mapped = map_mutate_error(err);
                                let mut response = build_edit_contract_error_response(
                                    &edit_args.path,
                                    mode,
                                    map_edit_contract_error(&mapped),
                                    mapped.message,
                                );
                                embed_display_payload(&mut response, &preview_display);
                                return Ok(response);
                            }
                        };

                        if summary.conflict {
                            let refreshed_plan =
                                match crate::tools::fs::core::plan_search_replace_edit(
                                    &resolved_path,
                                    edit_args.expected_version.as_deref(),
                                    &sr_op,
                                ) {
                                    Ok(refreshed_plan) => refreshed_plan,
                                    Err(err) => {
                                        let mapped = map_mutate_error(err);
                                        let mut response = build_edit_contract_error_response(
                                            &edit_args.path,
                                            mode,
                                            map_edit_contract_error(&mapped),
                                            mapped.message,
                                        );
                                        embed_display_payload(&mut response, &preview_display);
                                        return Ok(response);
                                    }
                                };
                            let mut response = build_edit_contract_response(
                                &edit_args.path,
                                mode,
                                refreshed_plan,
                                false,
                                None,
                            );
                            embed_display_payload(&mut response, &preview_display);
                            return Ok(response);
                        }

                        let mut response = build_edit_contract_response(
                            &edit_args.path,
                            mode,
                            plan,
                            true,
                            Some(&summary),
                        );
                        embed_display_payload(&mut response, &preview_display);
                        Ok(response)
                    }
                }
            }
            ResolvedEditOperation::Create { content } => {
                if resolved_path.parent().is_some_and(|p| !p.exists()) {
                    return Ok(build_edit_contract_error_response(
                        &edit_args.path,
                        mode,
                        "internal",
                        format!("Parent directory does not exist for '{}'", edit_args.path),
                    ));
                }

                let plan = match crate::tools::fs::core::plan_create_file(&resolved_path, &content)
                {
                    Ok(plan) => plan,
                    Err(err) => {
                        let mapped = map_mutate_error(err);
                        return Ok(build_edit_contract_error_response(
                            &edit_args.path,
                            mode,
                            map_edit_contract_error(&mapped),
                            mapped.message,
                        ));
                    }
                };

                match mode {
                    EditToolMode::Preview => Ok(build_edit_contract_response(
                        &edit_args.path,
                        mode,
                        plan,
                        false,
                        None,
                    )),
                    EditToolMode::Apply => {
                        let preview_display = super::pre_authorize::pre_authorize_fs_tool(
                            Some(super::builtin_kinds::BuiltinKind::Edit),
                            args,
                            cwd,
                        )
                        .and_then(|output| output.display)
                        .unwrap_or_else(|| edit_preview_display(&edit_args.path, &plan));

                        if plan.conflict {
                            let mut response = build_edit_contract_response(
                                &edit_args.path,
                                mode,
                                plan,
                                false,
                                None,
                            );
                            embed_display_payload(&mut response, &preview_display);
                            return Ok(response);
                        }

                        let summary = match crate::tools::fs::core::apply_create_file(
                            &resolved_path,
                            &content,
                        ) {
                            Ok(summary) => summary,
                            Err(err) => {
                                let mapped = map_mutate_error(err);
                                let mut response = build_edit_contract_error_response(
                                    &edit_args.path,
                                    mode,
                                    map_edit_contract_error(&mapped),
                                    mapped.message,
                                );
                                embed_display_payload(&mut response, &preview_display);
                                return Ok(response);
                            }
                        };

                        if summary.conflict {
                            let refreshed_plan = match crate::tools::fs::core::plan_create_file(
                                &resolved_path,
                                &content,
                            ) {
                                Ok(refreshed_plan) => refreshed_plan,
                                Err(err) => {
                                    let mapped = map_mutate_error(err);
                                    let mut response = build_edit_contract_error_response(
                                        &edit_args.path,
                                        mode,
                                        map_edit_contract_error(&mapped),
                                        mapped.message,
                                    );
                                    embed_display_payload(&mut response, &preview_display);
                                    return Ok(response);
                                }
                            };
                            let mut response = build_edit_contract_response(
                                &edit_args.path,
                                mode,
                                refreshed_plan,
                                false,
                                None,
                            );
                            embed_display_payload(&mut response, &preview_display);
                            return Ok(response);
                        }

                        let mut response = build_edit_contract_response(
                            &edit_args.path,
                            mode,
                            plan,
                            true,
                            Some(&summary),
                        );
                        embed_display_payload(&mut response, &preview_display);
                        Ok(response)
                    }
                }
            }
        }
    }
}

impl Previewable for EditTool {
    /// Pre-execution diff preview for the permission gate. Parses the edit
    /// arguments, resolves the operation, plans the change against the file
    /// on disk, and renders the unified diff. Returns `None` for arguments
    /// that do not form a valid apply-mode operation.
    fn preview(args: &JsonValue, cwd: &Path) -> Option<ToolDisplay> {
        let edit_args: EditArgs = serde_json::from_value(args.clone()).ok()?;
        let mode = parse_edit_mode(edit_args.mode.as_deref()).ok()?;
        if mode != EditToolMode::Apply {
            return None;
        }

        let operation = resolve_edit_operation(&edit_args).ok()?;
        let resolved_path = super::resolve_fs_path_for_cwd(&edit_args.path, cwd);
        let plan = match &operation {
            ResolvedEditOperation::SearchReplace(sr_op) => {
                let preview_version = match edit_args.expected_version.as_deref() {
                    Some(version) => Some(version.to_string()),
                    None => std::fs::read_to_string(&resolved_path)
                        .ok()
                        .map(|content| crate::tools::fs::core::version_token(&content)),
                };
                crate::tools::fs::core::plan_search_replace_edit(
                    &resolved_path,
                    preview_version.as_deref(),
                    sr_op,
                )
                .ok()?
            }
            ResolvedEditOperation::Create { content } => {
                if !resolved_path.parent().is_some_and(|p| p.exists()) {
                    return None;
                }
                crate::tools::fs::core::plan_create_file(&resolved_path, content).ok()?
            }
        };

        Some(edit_preview_display(&edit_args.path, &plan))
    }
}

#[cfg(test)]
#[path = "edit_test.rs"]
mod tests;
