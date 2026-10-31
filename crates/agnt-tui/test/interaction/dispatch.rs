//! Terminal-event dispatch test suite.
//!
//! The suite is split into topical sub-files. Shared imports, the `Result`
//! alias, and the helper re-exports live here; each sub-file pulls them in
//! with `use super::*;`.

// region:    --- Modules

mod abort_cancel;
mod agent_picker;
mod command_palette;
mod dispatch_support;
mod info_panels;
mod inline_slash;
mod mcp_panel;
mod mode_switching;
mod model_picker;
mod models_launcher;
mod permission_prompt;
mod submit_flow;
mod visual_mode;

use dispatch_support::*;

// endregion: --- Modules

use crate::{
    interaction::{
        cancel::CancelController,
        dispatch::dispatch_terminal_event,
        input::{TerminalEvent, TerminalKey, TerminalMouse},
        reducer::ESC_ABORT_CONFIRM_STATUS,
    },
    state::{
        ActivePicker, AppState, InfoPanel, InputMode, InputState, McpServerState,
        McpServerUsabilityState, PickerRenderKind, ScrollState, SwitchRequest, UiPhase,
    },
};
use agnt_core::protocol::contracts::SharedUiAction;
use agnt_core::protocol::event::PermissionDecision;
use ratatui::layout::Rect;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// Normal-mode state whose transcript pane covers the whole 80x20 area and
/// whose cursor sits mid-transcript, so a wheel notch is observable.
fn wheel_state() -> AppState {
    AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        scroll: ScrollState {
            viewport_height: 10,
            cursor_visual_row: 5,
            total_visual_rows: 30,
            transcript_area: Rect::new(0, 0, 80, 20),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn wheel_down() -> TerminalEvent {
    TerminalEvent::Mouse(TerminalMouse {
        up: false,
        column: 5,
        row: 5,
    })
}

// endregion: --- Test Support

// region:    --- Tests

#[test]
fn test_wheel_noop_under_modals() -> Result<()> {
    // -- Setup & Fixtures: no modal, the wheel scrolls.
    let mut plain = wheel_state();

    // -- Exec
    dispatch_terminal_event(&mut plain, &wheel_down(), None);

    // -- Check: the control case proves the hit test passes.
    assert_eq!(plain.scroll.cursor_visual_row, 8);

    // -- Setup & Fixtures: an info panel.
    let mut panel = wheel_state();
    panel.open_info_panel(InfoPanel::Help);

    // -- Exec
    dispatch_terminal_event(&mut panel, &wheel_down(), None);

    // -- Check
    assert!(panel.info_panel.is_some());
    assert_eq!(panel.info_panel_scroll, 0);
    assert_eq!(panel.scroll.cursor_visual_row, 5);

    // -- Setup & Fixtures: a picker.
    let mut picker = setup_agent_picker_open();
    picker.input.mode = InputMode::Normal;
    picker.scroll = wheel_state().scroll;

    // -- Exec
    dispatch_terminal_event(&mut picker, &wheel_down(), None);

    // -- Check
    assert!(picker.picker.active().is_some());
    assert_eq!(picker.scroll.cursor_visual_row, 5);

    // -- Setup & Fixtures: a permission prompt.
    let mut permission = wheel_state();
    open_permission_prompt(&mut permission);

    // -- Exec
    dispatch_terminal_event(&mut permission, &wheel_down(), None);

    // -- Check
    assert!(permission.permission.has_prompt());
    assert!(
        permission.permission.take_next_submission().is_none(),
        "the wheel must not submit a permission decision"
    );
    assert_eq!(permission.scroll.cursor_visual_row, 5);
    Ok(())
}

// endregion: --- Tests
