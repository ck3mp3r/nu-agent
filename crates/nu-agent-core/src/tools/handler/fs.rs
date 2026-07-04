use nu_plugin::EngineInterface;
use serde_json::Value as JsonValue;

use super::{
    ToolErrorKind, ToolHandlerError,
    result::{attach_display_payload, build_edit_preview_display},
    types::EditPreviewDisplayPayload,
};

#[derive(Debug, serde::Deserialize)]
pub struct ReadArgs {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

#[derive(Debug, serde::Deserialize)]
pub struct EditArgs {
    pub path: String,
    #[serde(default)]
    pub expected_version: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    pub operation: EditOperationArgs,
}

#[derive(Debug, serde::Deserialize, Clone)]
pub struct EditOperationArgs {
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
pub enum ResolvedEditOperation {
    SearchReplace(crate::tools::fs::core::EditOperation),
    Create { content: String },
}

#[derive(Debug, serde::Deserialize)]
struct PatchRangeArgs {
    start: usize,
    end: usize,
}

#[derive(Debug, serde::Deserialize)]
struct PatchOpArgs {
    range: PatchRangeArgs,
    replacement: String,
}

#[derive(Debug, serde::Deserialize)]
struct PatchArgs {
    path: String,
    #[serde(default)]
    expected_version: Option<String>,
    operations: Vec<PatchOpArgs>,
}

#[derive(Debug, serde::Deserialize)]
struct SkillArgs {
    name: String,
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

pub fn build_edit_preview_display_payload(
    path: &str,
    plan: &crate::tools::fs::core::EditPlan,
) -> EditPreviewDisplayPayload {
    let diff = crate::tools::fs::diff::compute_edit_unified_diff(
        std::path::Path::new("file"),
        &plan.previous_content,
        &plan.new_content,
    );

    EditPreviewDisplayPayload {
        path: path.to_string(),
        diff: diff.text,
        stats: crate::protocol::event::ToolDisplayStats {
            files_changed: Some(diff.stats.files_changed),
            insertions: Some(diff.stats.insertions),
            deletions: Some(diff.stats.deletions),
            diff_truncated: Some(diff.truncated),
            omitted_files: Some(diff.omitted_files),
            omitted_hunks: Some(diff.omitted_hunks),
        },
    }
}

pub fn resolve_edit_operation(args: &EditArgs) -> Result<ResolvedEditOperation, ToolHandlerError> {
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

pub fn map_edit_contract_error(error: &ToolHandlerError) -> &'static str {
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

fn map_mutate_error(error: crate::tools::fs::core::MutateError) -> ToolHandlerError {
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

pub fn resolve_fs_path_for_cwd(path: &str, cwd: &std::path::Path) -> std::path::PathBuf {
    let raw = std::path::Path::new(path);
    if raw.is_absolute() {
        raw.to_path_buf()
    } else {
        cwd.join(raw)
    }
}

pub fn resolve_fs_path(
    path: &str,
    engine: &EngineInterface,
) -> Result<std::path::PathBuf, ToolHandlerError> {
    let cwd = engine.get_current_dir().map_err(|e| ToolHandlerError {
        kind: ToolErrorKind::Runtime,
        message: format!("Unable to resolve current working directory: {e}"),
        details: None,
    })?;
    Ok(resolve_fs_path_for_cwd(path, std::path::Path::new(&cwd)))
}

pub fn dispatch_fs_tool(
    tool_name: &str,
    arguments: &JsonValue,
    cwd: &std::path::Path,
) -> Result<Option<JsonValue>, ToolHandlerError> {
    use crate::tools::fs::core::{
        PatchOp, PatchRange, ReadRequest, apply_line_range_patch_batch, apply_search_replace_edit,
        read_file,
    };

    match tool_name {
        "read" => {
            let args: ReadArgs =
                serde_json::from_value(arguments.clone()).map_err(|e| ToolHandlerError {
                    kind: ToolErrorKind::Validation,
                    message: format!("Invalid read arguments: {e}"),
                    details: None,
                })?;

            let resolved_path = resolve_fs_path_for_cwd(&args.path, cwd);
            let response = read_file(
                &resolved_path,
                ReadRequest {
                    offset: args.offset,
                    limit: args.limit,
                },
            )
            .map_err(|e| ToolHandlerError {
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
            let args: EditArgs =
                serde_json::from_value(arguments.clone()).map_err(|e| ToolHandlerError {
                    kind: ToolErrorKind::Validation,
                    message: format!("Invalid edit arguments: {e}"),
                    details: None,
                })?;

            let resolved_path = resolve_fs_path_for_cwd(&args.path, cwd);
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

            match operation {
                ResolvedEditOperation::SearchReplace(sr_op) => {
                    let plan = match crate::tools::fs::core::plan_search_replace_edit(
                        &resolved_path,
                        args.expected_version.as_deref(),
                        &sr_op,
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
                        EditToolMode::Preview => Ok(Some(build_edit_contract_response(
                            &args.path, mode, plan, false, None,
                        ))),
                        EditToolMode::Apply => {
                            let preview_display = super::pre_authorize::pre_authorize_fs_tool(
                                tool_name, arguments, cwd,
                            )
                            .and_then(|output| output.display)
                            .unwrap_or_else(|| {
                                build_edit_preview_display(build_edit_preview_display_payload(
                                    &args.path, &plan,
                                ))
                            });

                            if plan.conflict || !plan.would_change {
                                let mut response = build_edit_contract_response(
                                    &args.path, mode, plan, false, None,
                                );
                                attach_display_payload(&mut response, &preview_display);
                                return Ok(Some(response));
                            }

                            let summary = match apply_search_replace_edit(
                                &resolved_path,
                                args.expected_version.as_deref(),
                                &sr_op,
                            ) {
                                Ok(summary) => summary,
                                Err(crate::tools::fs::core::MutateError::Conflict(_)) => {
                                    let refreshed_plan =
                                        match crate::tools::fs::core::plan_search_replace_edit(
                                            &resolved_path,
                                            args.expected_version.as_deref(),
                                            &sr_op,
                                        ) {
                                            Ok(refreshed_plan) => refreshed_plan,
                                            Err(err) => {
                                                let mapped = map_mutate_error(err);
                                                let mut response =
                                                    build_edit_contract_error_response(
                                                        &args.path,
                                                        mode,
                                                        map_edit_contract_error(&mapped),
                                                        mapped.message,
                                                    );
                                                attach_display_payload(
                                                    &mut response,
                                                    &preview_display,
                                                );
                                                return Ok(Some(response));
                                            }
                                        };
                                    let mut response = build_edit_contract_response(
                                        &args.path,
                                        mode,
                                        refreshed_plan,
                                        false,
                                        None,
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
                                let refreshed_plan =
                                    match crate::tools::fs::core::plan_search_replace_edit(
                                        &resolved_path,
                                        args.expected_version.as_deref(),
                                        &sr_op,
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
                                    None,
                                );
                                attach_display_payload(&mut response, &preview_display);
                                return Ok(Some(response));
                            }

                            let mut response = build_edit_contract_response(
                                &args.path,
                                mode,
                                plan,
                                true,
                                Some(&summary),
                            );
                            attach_display_payload(&mut response, &preview_display);
                            Ok(Some(response))
                        }
                    }
                }
                ResolvedEditOperation::Create { content } => {
                    if resolved_path.parent().is_some_and(|p| !p.exists()) {
                        return Ok(Some(build_edit_contract_error_response(
                            &args.path,
                            mode,
                            "internal",
                            format!("Parent directory does not exist for '{}'", args.path),
                        )));
                    }

                    let plan =
                        match crate::tools::fs::core::plan_create_file(&resolved_path, &content) {
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
                        EditToolMode::Preview => Ok(Some(build_edit_contract_response(
                            &args.path, mode, plan, false, None,
                        ))),
                        EditToolMode::Apply => {
                            let preview_display = super::pre_authorize::pre_authorize_fs_tool(
                                tool_name, arguments, cwd,
                            )
                            .and_then(|output| output.display)
                            .unwrap_or_else(|| {
                                build_edit_preview_display(build_edit_preview_display_payload(
                                    &args.path, &plan,
                                ))
                            });

                            if plan.conflict {
                                let mut response = build_edit_contract_response(
                                    &args.path, mode, plan, false, None,
                                );
                                attach_display_payload(&mut response, &preview_display);
                                return Ok(Some(response));
                            }

                            let summary = match crate::tools::fs::core::apply_create_file(
                                &resolved_path,
                                &content,
                            ) {
                                Ok(summary) => summary,
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
                                let refreshed_plan = match crate::tools::fs::core::plan_create_file(
                                    &resolved_path,
                                    &content,
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
                                    None,
                                );
                                attach_display_payload(&mut response, &preview_display);
                                return Ok(Some(response));
                            }

                            let mut response = build_edit_contract_response(
                                &args.path,
                                mode,
                                plan,
                                true,
                                Some(&summary),
                            );
                            attach_display_payload(&mut response, &preview_display);
                            Ok(Some(response))
                        }
                    }
                }
            }
        }
        "patch" => {
            let args: PatchArgs =
                serde_json::from_value(arguments.clone()).map_err(|e| ToolHandlerError {
                    kind: ToolErrorKind::Validation,
                    message: format!("Invalid patch arguments: {e}"),
                    details: None,
                })?;

            let resolved_path = resolve_fs_path_for_cwd(&args.path, cwd);
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
            let args: SkillArgs =
                serde_json::from_value(arguments.clone()).map_err(|e| ToolHandlerError {
                    kind: ToolErrorKind::Validation,
                    message: format!("Invalid skill arguments: {e}"),
                    details: None,
                })?;

            let resolved =
                crate::protocol::skills::resolve_explicit_skill_request_for_cwd(cwd, &args.name)
                    .map_err(|e| ToolHandlerError {
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
