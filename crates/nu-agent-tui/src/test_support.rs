use std::path::{Path, PathBuf};

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
            sort_key: Vec::new(),
            payload: PickerPayload::Command(*a),
        })
        .collect();
}

// region:    --- Test Support (workspace architecture invariant greps)

/// Root of the workspace crate directory (…/nu-agent/crates), resolved from
/// CARGO_MANIFEST_DIR so the walker is independent of the process CWD.
fn crates_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .map(|workspace| workspace.join("crates"))
        .expect("manifest must live in <workspace>/crates/<crate>")
}

/// Every `.rs` file under `crates/`, sorted for deterministic failure output.
/// Excludes nothing: the invariant greps run over production AND test sources
/// so the architectural rules hold at both layers — except the invariant
/// test itself, whose source embeds the ban patterns as literals.
pub(crate) fn workspace_rs_files() -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![crates_root()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs")
                && path.file_name().and_then(|e| e.to_str()) != Some("architecture_test.rs")
            {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// First file containing `pattern` as a plain-text substring, formatted for
/// a failure message. The substring form matches the plain `grep -r` the
/// task criteria are written against; callers quote the pattern accordingly.
pub(crate) fn first_file_containing(files: &[PathBuf], pattern: &str) -> Option<String> {
    for path in files {
        let Ok(content) = std::fs::read_to_string(path) else {
            continue;
        };
        if content.contains(pattern) {
            return Some(path.display().to_string());
        }
    }
    None
}

// endregion: --- Test Support (workspace architecture invariant greps)
