//! Free-standing wire-shape converters: crate data in, ACP schema out.

use agent_client_protocol::schema::v1::{
    Content, Diff, SessionInfoUpdate, SessionUpdate, ToolCallContent, ToolKind,
};
use agnt_kernel::{BuiltinKind, CompactionSource, ToolKind as KernelToolKind};
use agnt_render::transcript::ir::{ContentKind, Display, DisplaySection};

/// Map the kernel's tool kind to an ACP tool kind.
///
/// The kernel already classified the call: its name became a variant once, at
/// the boundary that produced the record. This is a total map over that enum,
/// so no name — and no name prefix — is read here.
pub(super) fn tool_kind(kind: KernelToolKind) -> ToolKind {
    match kind {
        KernelToolKind::Builtin(
            BuiltinKind::Read
            | BuiltinKind::AstQuery
            | BuiltinKind::AstNodes
            | BuiltinKind::AstRefs
            | BuiltinKind::AstTree,
        ) => ToolKind::Read,
        KernelToolKind::Builtin(BuiltinKind::Edit) => ToolKind::Edit,
        KernelToolKind::Builtin(BuiltinKind::Grep | BuiltinKind::Glob) => ToolKind::Search,
        KernelToolKind::Builtin(
            BuiltinKind::Nu
            | BuiltinKind::TmuxSession
            | BuiltinKind::TmuxWindow
            | BuiltinKind::TmuxPane
            | BuiltinKind::TmuxLayout,
        ) => ToolKind::Execute,
        KernelToolKind::Builtin(BuiltinKind::Http) => ToolKind::Fetch,
        KernelToolKind::Builtin(
            BuiltinKind::Skill
            | BuiltinKind::SpawnAgent
            | BuiltinKind::TerminateAgent
            | BuiltinKind::SendMessage
            | BuiltinKind::ListAgents,
        )
        | KernelToolKind::Other => ToolKind::Other,
    }
}

/// The label this translator puts on a compaction's source.
fn source_label(source: CompactionSource) -> &'static str {
    match source {
        CompactionSource::Auto => "auto",
        CompactionSource::Manual => "slash",
    }
}

/// The head of a compaction summary: the client previews it, never the body.
pub(super) fn summary_preview(summary: &str) -> String {
    summary.chars().take(200).collect()
}

/// Build a `session_info_update` carrying compaction state in `_meta`.
///
/// A record that names no source carries no `source` key: the client is told
/// what the record says, and the absence stays an absence.
pub(super) fn compaction_update(
    state: &str,
    source: Option<CompactionSource>,
    detail: Option<String>,
) -> SessionUpdate {
    let mut compaction = serde_json::Map::new();
    compaction.insert(
        "state".to_string(),
        serde_json::Value::String(state.to_string()),
    );
    if let Some(source) = source {
        compaction.insert(
            "source".to_string(),
            serde_json::Value::String(source_label(source).to_string()),
        );
    }
    if let Some(detail) = detail {
        compaction.insert("detail".to_string(), serde_json::Value::String(detail));
    }
    let mut meta = serde_json::Map::new();
    meta.insert(
        "nu-agent".to_string(),
        serde_json::Value::Object(compaction),
    );
    SessionUpdate::SessionInfoUpdate(SessionInfoUpdate::new().meta(meta))
}

/// Map every display section to ACP tool-call content.
pub(crate) fn content_from_display(display: &Display) -> Vec<ToolCallContent> {
    display.sections.iter().map(section_content).collect()
}

/// Map one display section to ACP tool-call content.
///
/// A diff section becomes an ACP `Diff` carrying the file path from the
/// content kind. The section content is a unified diff, so the old and new
/// sides are reconstructed from the hunk lines — the reconstruction covers the
/// hunks only, not the untouched remainder of the file.
fn section_content(section: &DisplaySection) -> ToolCallContent {
    match &section.kind {
        ContentKind::Diff { path, .. } => {
            let (old_text, new_text) = split_unified_diff(&section.content);
            ToolCallContent::Diff(Diff::new(path.clone(), new_text).old_text(old_text))
        }
        _ => ToolCallContent::Content(Content::new(section.content.clone())),
    }
}

/// Split a unified diff into its old and new sides.
///
/// File headers, hunk headers, and `\ No newline` markers are dropped. Context
/// lines feed both sides; `-` lines feed the old side, `+` lines the new side.
/// The old side is `None` when the diff contains no old-side line (a new file).
fn split_unified_diff(text: &str) -> (Option<String>, String) {
    let mut old_text = String::new();
    let mut new_text = String::new();
    let mut has_old = false;
    for line in text.lines() {
        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("@@") {
            continue;
        }
        if line.starts_with('\\') {
            continue;
        }
        match line.chars().next() {
            Some('-') => {
                old_text.push_str(&line[1..]);
                old_text.push('\n');
                has_old = true;
            }
            Some('+') => {
                new_text.push_str(&line[1..]);
                new_text.push('\n');
            }
            Some(' ') => {
                old_text.push_str(&line[1..]);
                old_text.push('\n');
                new_text.push_str(&line[1..]);
                new_text.push('\n');
                has_old = true;
            }
            _ => {
                old_text.push_str(line);
                old_text.push('\n');
                new_text.push_str(line);
                new_text.push('\n');
                has_old = true;
            }
        }
    }
    (has_old.then_some(old_text), new_text)
}
