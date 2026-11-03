use std::path::PathBuf;

use agnt_render::tool_view::ToolView;
use agnt_render::transcript::entry::TranscriptEntry;

use crate::state::{ActivePicker, AppState, CommandPaletteAction, PickerOption, PickerPayload};

/// Flatten one entry to plain text: the markdown body, the tool call line,
/// the task call line, or the notice/banner text.
pub(crate) fn entry_text(entry: &TranscriptEntry) -> String {
    match entry {
        TranscriptEntry::Message(message) => message.markdown.clone(),
        TranscriptEntry::Tool(tool) => tool.call.call_line(),
        TranscriptEntry::Task(task) => format!("{} · {}", task.name, task.summary),
        TranscriptEntry::Notice(notice) => notice.text.clone(),
        TranscriptEntry::Banner(banner) => banner.text.clone(),
    }
}

pub(crate) fn markdown_fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../agnt-render/src/fixtures/markdown")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!(
            "failed to read markdown fixture {}: {error}",
            path.display()
        )
    })
}

pub(crate) fn open_command_palette_for_test(state: &mut AppState) {
    state.info_panel = None;
    let entry = state.picker.open(ActivePicker::CommandPalette);
    entry.state.options = CommandPaletteAction::PALETTE_ACTIONS
        .iter()
        .map(|a| PickerOption {
            id: a.label().to_string(),
            display: a.label().to_string(),
            search_text: a.label().to_string(),
            section: None,
            sort_key: Vec::new(),
            payload: PickerPayload::Command(*a),
        })
        .collect();
}
