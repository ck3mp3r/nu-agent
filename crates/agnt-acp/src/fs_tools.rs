//! ACP filesystem tools that route through the client's `fs/*` capabilities.
//!
//! When the ACP client advertises `fs.readTextFile`, the session runtime
//! registers these tools instead of the local-filesystem built-ins, so every
//! read reaches the editor's buffer rather than the agent's disk.

// region:    --- Modules

use std::path::{Path, PathBuf};

use agent_client_protocol::schema::v1::{ReadTextFileRequest, SessionId, WriteTextFileRequest};
use agent_client_protocol::{Client, ConnectionTo};
use agnt_core::tools::fs::core::{
    EditMatchMode, EditOccurrence, EditOperation, EditPlan, plan_create_from_content,
    plan_search_replace_from_content, split_lines_preserving_terminators, version_token,
};
use agnt_core::tools::fs::diff::compute_edit_unified_diff;
use agnt_core::tools::handler::edit::{EditToolMode, parse_edit_mode};
use agnt_core::tools::handler::resolve_fs_path_for_cwd;
use agnt_core::tools::limits::truncate_tool_output;
use agnt_core::types::{DynamicTool, ToolDefinition, ToolExecutionError, ToolOutput};

// endregion: --- Modules

// region:    --- Types

/// Arguments of the ACP `read` tool. Mirrors the standard read tool's schema.
#[derive(Debug, serde::Deserialize)]
struct ReadArgs {
    path: String,
    #[serde(default)]
    offset: Option<usize>,
    #[serde(default)]
    limit: Option<usize>,
}

/// Arguments of the ACP `edit` tool. Mirrors the standard edit tool's schema.
#[derive(Debug, serde::Deserialize)]
struct EditArgs {
    path: String,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    expected_version: Option<String>,
    operation: EditOperationArgs,
}

/// The `operation` object of the ACP `edit` tool arguments.
#[derive(Debug, serde::Deserialize)]
struct EditOperationArgs {
    #[serde(default, rename = "type")]
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

/// A validated edit operation, resolved from [`EditOperationArgs`].
enum ResolvedEditOperation {
    SearchReplace(EditOperation),
    Create { content: String },
}

// endregion: --- Types

// region:    --- Public Functions

/// Build the ACP `read` tool: a [`DynamicTool`] that reads through the client's
/// `fs/read_text_file` capability instead of the local filesystem.
///
/// The tool keeps the standard read tool's name, description, and parameters,
/// so the model cannot tell the two apart. The returned JSON has the same shape
/// as [`agnt_core::tools::handler::read::ReadTool`]: `path`, `content`,
/// `total_lines`, `offset`, `limit`, and `version`.
pub fn make_acp_read_tool(
    def: ToolDefinition,
    cwd: PathBuf,
    max_bytes: usize,
    connection: ConnectionTo<Client>,
    session_id: String,
) -> DynamicTool {
    let name = def.name.clone();
    let description = def.description.clone();
    let parameters = def.parameters.clone();
    DynamicTool::new(name, description, parameters, move |args| {
        let cwd = cwd.clone();
        let connection = connection.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            let args: ReadArgs = serde_json::from_value(args).map_err(|e| {
                ToolExecutionError::invalid_args(format!("Invalid read arguments: {e}"))
            })?;

            let resolved_path = resolve_fs_path_for_cwd(&args.path, &cwd);
            log::info!(
                "ACP read tool: path={} resolved={}",
                args.path,
                resolved_path.display()
            );
            let request = ReadTextFileRequest::new(SessionId::new(session_id), resolved_path);
            let response = ConnectionTo::<Client>::send_request(&connection, request)
                .block_task()
                .await
                .map_err(|e| {
                    log::warn!("ACP read tool: fs/read_text_file failed: {e}");
                    ToolExecutionError::other(format!("read failed: {e}"))
                })?;
            log::info!("ACP read tool: got {} bytes", response.content.len());

            let content = response.content;
            let total_lines = split_lines_preserving_terminators(&content).len();
            let version = version_token(&content);
            let sliced = match (args.offset, args.limit) {
                (Some(offset), Some(limit)) => split_lines_preserving_terminators(&content)
                    .into_iter()
                    .skip(offset)
                    .take(limit)
                    .collect::<String>(),
                _ => content.clone(),
            };

            let result = serde_json::json!({
                "path": args.path,
                "content": sliced,
                "total_lines": total_lines,
                "offset": args.offset,
                "limit": args.limit,
                "version": version,
            });
            let json_str = serde_json::to_string(&result).map_err(|e| {
                ToolExecutionError::other(format!("JSON serialization failed: {e}"))
            })?;
            Ok(ToolOutput::text(truncate_tool_output(json_str, max_bytes)))
        })
    })
}

/// Build the ACP `edit` tool: a [`DynamicTool`] that reads and writes through
/// the client's `fs/read_text_file` and `fs/write_text_file` capabilities
/// instead of the local filesystem.
///
/// The tool keeps the standard edit tool's name, description, and parameters,
/// so the model cannot tell the two apart. The returned JSON has the same shape
/// as [`agnt_core::tools::handler::edit::EditTool`]: `path`, `mode`, `applied`,
/// `would_change`, `diff`, `stats`, `diagnostics`, `changed`, `replacements`,
/// `wrote`, `noop`, `conflict`, `expected_version`, `previous_version`, and
/// `new_version`.
pub fn make_acp_edit_tool(
    def: ToolDefinition,
    cwd: PathBuf,
    max_bytes: usize,
    connection: ConnectionTo<Client>,
    session_id: String,
) -> DynamicTool {
    let name = def.name.clone();
    let description = def.description.clone();
    let parameters = def.parameters.clone();
    DynamicTool::new(name, description, parameters, move |args| {
        let cwd = cwd.clone();
        let connection = connection.clone();
        let session_id = session_id.clone();
        Box::pin(async move {
            let args: EditArgs = serde_json::from_value(args).map_err(|e| {
                ToolExecutionError::invalid_args(format!("Invalid edit arguments: {e}"))
            })?;

            let mode = parse_edit_mode(args.mode.as_deref())
                .map_err(|e| ToolExecutionError::invalid_args(e.message))?;
            let mode_str = match mode {
                EditToolMode::Preview => "preview",
                EditToolMode::Apply => "apply",
            };

            let operation = resolve_edit_operation(&args.operation)?;
            let resolved_path = resolve_fs_path_for_cwd(&args.path, &cwd);
            log::info!("ACP edit tool: path={} mode={}", args.path, mode_str);

            let plan = match operation {
                ResolvedEditOperation::SearchReplace(op) => {
                    let content =
                        read_client_file(&connection, &session_id, &resolved_path).await?;
                    let current_version = version_token(&content);
                    let expected = args.expected_version.as_deref().unwrap_or(&current_version);
                    plan_search_replace_from_content(&content, expected, &op).map_err(|e| {
                        ToolExecutionError::invalid_args(format!("edit failed: {e}"))
                    })?
                }
                ResolvedEditOperation::Create { content } => {
                    let existing = read_client_file(&connection, &session_id, &resolved_path)
                        .await
                        .ok();
                    plan_create_from_content(&content, existing)
                }
            };

            let applied = if mode == EditToolMode::Apply && !plan.conflict && plan.would_change {
                write_client_file(&connection, &session_id, &resolved_path, &plan.new_content)
                    .await?;
                true
            } else {
                false
            };

            let result = build_edit_response(&args.path, mode_str, plan, applied);
            let json_str = serde_json::to_string(&result).map_err(|e| {
                ToolExecutionError::other(format!("JSON serialization failed: {e}"))
            })?;
            Ok(ToolOutput::text(truncate_tool_output(json_str, max_bytes)))
        })
    })
}

// endregion: --- Public Functions

// region:    --- Support

/// Resolve the `operation` object into a validated edit operation.
fn resolve_edit_operation(
    operation: &EditOperationArgs,
) -> Result<ResolvedEditOperation, ToolExecutionError> {
    let op_type = operation
        .operation_type
        .as_deref()
        .unwrap_or("search_replace");
    match op_type {
        "create" => {
            let content = operation.content.clone().ok_or_else(|| {
                ToolExecutionError::invalid_args(
                    "Invalid edit arguments: missing field `operation.content` for create operation",
                )
            })?;
            Ok(ResolvedEditOperation::Create { content })
        }
        "search_replace" => {
            let search = operation.search.clone().ok_or_else(|| {
                ToolExecutionError::invalid_args(
                    "Invalid edit arguments: missing field `operation.search` for search_replace operation",
                )
            })?;
            let replacement = operation.replacement.clone().ok_or_else(|| {
                ToolExecutionError::invalid_args(
                    "Invalid edit arguments: missing field `operation.replacement` for search_replace operation",
                )
            })?;
            let match_mode = match operation.match_mode.as_deref().unwrap_or("literal") {
                "literal" => EditMatchMode::Literal,
                "regex" => EditMatchMode::Regex,
                other => {
                    return Err(ToolExecutionError::invalid_args(format!(
                        "Invalid edit.match_mode '{other}': expected 'literal' or 'regex'"
                    )));
                }
            };
            let occurrence = match operation.occurrence.as_deref().unwrap_or("first") {
                "first" => EditOccurrence::First,
                "all" => EditOccurrence::All,
                other => {
                    return Err(ToolExecutionError::invalid_args(format!(
                        "Invalid edit.occurrence '{other}': expected 'first' or 'all'"
                    )));
                }
            };
            Ok(ResolvedEditOperation::SearchReplace(EditOperation {
                search,
                replacement,
                match_mode,
                occurrence,
            }))
        }
        other => Err(ToolExecutionError::invalid_args(format!(
            "Invalid edit.operation.type '{other}': expected 'search_replace' or 'create'"
        ))),
    }
}

/// Read `path` through the client's `fs/read_text_file` capability.
async fn read_client_file(
    connection: &ConnectionTo<Client>,
    session_id: &str,
    path: &Path,
) -> Result<String, ToolExecutionError> {
    let request = ReadTextFileRequest::new(SessionId::new(session_id), path);
    let response = ConnectionTo::<Client>::send_request(connection, request)
        .block_task()
        .await
        .map_err(|e| {
            log::warn!(
                "ACP fs/read_text_file failed: path={} err={}",
                path.display(),
                e
            );
            ToolExecutionError::other(format!("read failed: {e}"))
        })?;
    log::info!(
        "ACP fs/read_text_file ok: path={} bytes={}",
        path.display(),
        response.content.len()
    );
    Ok(response.content)
}

/// Write `content` to `path` through the client's `fs/write_text_file` capability.
async fn write_client_file(
    connection: &ConnectionTo<Client>,
    session_id: &str,
    path: &Path,
    content: &str,
) -> Result<(), ToolExecutionError> {
    let request = WriteTextFileRequest::new(SessionId::new(session_id), path, content);
    ConnectionTo::<Client>::send_request(connection, request)
        .block_task()
        .await
        .map_err(|e| {
            log::warn!(
                "ACP fs/write_text_file failed: path={} err={}",
                path.display(),
                e
            );
            ToolExecutionError::other(format!("write failed: {e}"))
        })?;
    log::info!(
        "ACP fs/write_text_file ok: path={} bytes={}",
        path.display(),
        content.len()
    );
    Ok(())
}

/// Build the edit response JSON, matching the standard edit tool's field shape.
fn build_edit_response(path: &str, mode: &str, plan: EditPlan, applied: bool) -> serde_json::Value {
    let diff =
        compute_edit_unified_diff(Path::new(path), &plan.previous_content, &plan.new_content);
    let changed = plan.would_change;
    let wrote = applied && plan.would_change;

    let mut diagnostics = Vec::new();
    if plan.conflict {
        diagnostics.push(serde_json::json!({
            "class": "stale",
            "message": format!(
                "stale expected_version '{}' (current '{}')",
                plan.expected_version, plan.previous_version
            ),
        }));
    }

    serde_json::json!({
        "path": path,
        "mode": mode,
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
        "noop": plan.noop,
        "conflict": plan.conflict,
        "expected_version": plan.expected_version,
        "previous_version": plan.previous_version,
        "new_version": plan.new_version,
    })
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../test/fs_tools.rs"]
mod fs_tools_test;

// endregion: --- Tests
