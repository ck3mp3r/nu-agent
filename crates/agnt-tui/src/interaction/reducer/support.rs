//! The actions the lanes call: the escape coupling, quit, the mode entries, the
//! yank, and the palette toggle — plus the status strings they print.

use std::time::Instant;

use crate::interaction::cancel::CancelController;
use crate::state::{
    ActivePicker, AppState, CommandPaletteAction, InputMode, PickerOption, PickerPayload,
    PickerRenderKind, UiPhase,
};

pub(crate) const ESC_ABORT_CONFIRM_STATUS: &str = "Esc again to cancel";

pub(crate) const ESC_SETTLED_STATUS: &str = "turn already finished — nothing to cancel";

pub(crate) const VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS: &str =
    "Visual mode requires transcript focus (Tab/h/l).";

/// The escape coupling: one press does one thing by phase.
///
/// A press that closes an open surface ends at the surface: it never arms,
/// never confirms, and never touches the mode, the status bar, or the settle
/// notice. A bare press on a running turn arms the abort, and a bare press
/// inside the arm's message lifetime executes it. A bare press that finds an
/// expired arm drops it silently and re-arms — the message reappears and no
/// cancel fires out of nowhere. Idle keeps the plain behavior: a settled abort
/// reports the notice that its lost press is owed, otherwise the mode exits.
pub(super) fn handle_escape(
    state: &mut AppState,
    cancel_controller: Option<&CancelController>,
) -> bool {
    if state.info_panel.is_some() {
        state.close_info_panel();
        return true;
    }

    if state.phase == UiPhase::Idle {
        if state.abort.settle_notice {
            state.abort.settle_notice = false;
            state.status.message.set_message(ESC_SETTLED_STATUS);
            return true;
        }
        if state.input.mode == InputMode::Insert {
            state.enter_normal_mode();
            return true;
        }
        if state.input.mode == InputMode::Visual {
            state.scroll.selection = None;
            state.enter_normal_mode();
            return true;
        }
        return false;
    }

    if state.phase == UiPhase::AbortPending && state.abort.pending {
        if state.abort.arm_live() {
            return handle_escape_confirm(state, cancel_controller);
        }
        let _ = state.sweep_expired_abort(Instant::now());
    }
    if state.request_abort_confirmation() {
        state.status.message.set_message(ESC_ABORT_CONFIRM_STATUS);
        return true;
    }
    false
}

fn handle_escape_confirm(
    state: &mut AppState,
    cancel_controller: Option<&CancelController>,
) -> bool {
    if state.phase == UiPhase::AbortPending
        && state.abort.pending
        && state.is_active_cycle()
        && state.abort.arm_live()
    {
        if let Some(controller) = cancel_controller {
            controller.request_cancel();
        }
        state.cancel_and_restore_pending_to_input();
        state.permission.close_prompt();
        state.enter_insert_mode();
        state.status.message.clear();
        return true;
    }
    false
}

pub(super) fn handle_quit(
    state: &mut AppState,
    cancel_controller: Option<&CancelController>,
) -> bool {
    if state.phase != crate::state::UiPhase::Idle {
        // Busy: cancel the running turn and quit
        if let Some(controller) = cancel_controller {
            controller.request_cancel();
        }
        state.quit_requested = true;
    } else {
        // Idle: quit
        state.request_quit_if_idle();
    }
    true
}

pub(super) fn handle_enter_insert_mode(state: &mut AppState) -> bool {
    if state.phase == UiPhase::Idle {
        state.enter_insert_mode();
        return true;
    }
    false
}

pub(super) fn handle_enter_visual_mode(state: &mut AppState) -> bool {
    if state.phase != UiPhase::Idle {
        return false;
    }
    // Scroll domain: focus guard, selection start, status lines. The input
    // mode transition stays here (input domain).
    let entered = state.scroll.enter_visual_mode(&mut state.status);
    if entered {
        state.input.mode = InputMode::Visual;
    }
    true
}

pub(super) fn handle_enter_normal_mode_from_chord(state: &mut AppState) -> bool {
    if state.phase == UiPhase::Idle {
        state.enter_normal_mode();
        return true;
    }
    false
}

pub(super) fn handle_yank_selection(state: &mut AppState) -> bool {
    if state.input.mode != InputMode::Visual {
        return false;
    }
    // Cross-domain: the payload comes from the transcript model (transcript +
    // scroll + theme domains), the clipboard request stays here (input domain).
    if let Some(payload) = state.yank_selection_payload() {
        state.input.set_clipboard_request(payload);
    }
    state.enter_normal_mode();
    true
}

pub(super) fn handle_toggle_command_palette(state: &mut AppState) -> bool {
    if state.picker.render_kind() == Some(PickerRenderKind::CommandPalette) {
        state.picker.close();
    } else {
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
    true
}
