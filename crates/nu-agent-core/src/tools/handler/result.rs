use serde_json::Value as JsonValue;

use super::builtin_kinds::BuiltinKind;
use super::types::EditPreviewDisplayPayload;
use crate::protocol::event::{ToolDisplay, ToolDisplaySection, ToolDisplayStats};

pub fn build_edit_preview_display(preview: EditPreviewDisplayPayload) -> ToolDisplay {
    ToolDisplay {
        title: format!("edit {}", preview.path),
        sections: vec![ToolDisplaySection {
            label: preview.path,
            language: "diff".to_string(),
            content: preview.diff,
            stats: Some(preview.stats),
        }],
    }
}

pub fn attach_display_payload(response: &mut JsonValue, display: &ToolDisplay) {
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
                JsonValue::String(section.language.clone()),
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

pub fn build_direct_tool_display(tool_name: &str, payload: &JsonValue) -> Option<ToolDisplay> {
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
            language: "diff".to_string(),
            content: diff,
            stats: parse_display_stats(payload.get("stats")),
        }],
    })
}
