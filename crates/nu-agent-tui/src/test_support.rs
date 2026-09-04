use std::path::PathBuf;

use crate::state::{ActivePicker, AppState, CommandPaletteAction, PickerOption, PickerPayload};

pub(crate) fn markdown_fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../nu-agent-core/src/fixtures/markdown")
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
            payload: PickerPayload::Command(*a),
        })
        .collect();
}
