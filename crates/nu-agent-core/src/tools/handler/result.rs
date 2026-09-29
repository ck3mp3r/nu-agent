use serde_json::Value as JsonValue;

use super::builtin_kinds::BuiltinKind;
use crate::protocol::event::{ToolDisplay, ToolDisplaySection, ToolDisplayStats};
use crate::transcript::ir::ContentKind;

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

/// Parse the `language` string on a legacy persisted display section into a
/// `ContentKind`. Diff languages map to `ContentKind::Diff`, everything else
/// to `ContentKind::Code`; the empty string maps to `Plain`.
fn content_kind_from_language(language: &str) -> ContentKind {
    match language {
        "" => ContentKind::Plain,
        "diff" => ContentKind::Diff {
            language: language.to_string(),
        },
        other => ContentKind::Code {
            language: other.to_string(),
        },
    }
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
            kind: content_kind_from_language(section.get("language")?.as_str()?),
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

/// Extract a tool display from a tool result JSON. An explicit `display`
/// object wins; otherwise edit results get a synthesized diff display.
pub fn tool_display_from_result(tool_name: &str, payload: &JsonValue) -> Option<ToolDisplay> {
    if let Some(explicit_display) = payload.get("display")
        && let Some(display) = tool_display_from_minimal_object(explicit_display)
    {
        return Some(display);
    }

    let kind = tool_name.parse::<BuiltinKind>().ok();
    match kind {
        Some(BuiltinKind::Edit) => {}
        _ => return None,
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
            kind: ContentKind::Diff {
                language: "diff".to_string(),
            },
            content: diff,
            stats: parse_display_stats(payload.get("stats")),
        }],
    })
}
