//! The key lane: the input-boundary lens, the vim keymaps, the shared keys, and
//! the wheel gates.

use crate::interaction::cancel::CancelController;
use crate::interaction::dispatch::KeyLens;
use crate::interaction::input::Key;
use crate::state::{ActivePicker, AppState, InputMode, ScrollAction, UiPhase};
use agnt_runtime::protocol::event::PermissionDecision;

use super::submit::{handle_picker_submit, handle_submit};
use super::support::{
    handle_enter_insert_mode, handle_enter_normal_mode_from_chord, handle_enter_visual_mode,
    handle_escape, handle_quit, handle_toggle_command_palette, handle_yank_selection,
};

// region:    --- Key arms

/// Fold one key: the input-boundary lens runs first (open panel, open picker,
/// the permission card, the agent chord); what it leaves behind is the vim
/// keymap, the insert-exit chords, and the pending `gg` key.
pub(super) fn reduce_key(
    state: &mut AppState,
    key: Key,
    cancel_controller: Option<&CancelController>,
) -> bool {
    // The input lock drops the editing keys the coordinator owns while a turn
    // runs. Enter is not one of them: a prompt typed during the turn must queue
    // instead of vanishing. The card's own keys stay reachable, so a decision
    // can be taken while the input is locked.
    let locked = state.input_locked && !state.permission.has_prompt();
    if locked && edits_input(key) {
        return false;
    }

    match crate::interaction::dispatch::rewrite_action(state, key) {
        KeyLens::Consumed(changed) => changed,
        KeyLens::Scroll(action) => state.scroll.reduce_scroll_action(action),
        KeyLens::PickerSubmit(submit) => handle_picker_submit(state, submit),
        KeyLens::Decision(decision) => reduce_permission_decided(state, decision),
        KeyLens::Quit => handle_quit(state, cancel_controller),
        KeyLens::Escape => handle_escape(state, cancel_controller),
        KeyLens::Pass => reduce_keymap(state, key, cancel_controller),
    }
}

pub(super) fn reduce_permission_decided(
    state: &mut AppState,
    decision: PermissionDecision,
) -> bool {
    if state.permission.submit_decision(decision) {
        state.ensure_invariants();
    }
    true
}

/// The keys the input lock drops: every key that edits the draft without
/// queueing it. Enter is absent on purpose — it submits, so a prompt typed
/// during a turn reaches the queue.
fn edits_input(key: Key) -> bool {
    matches!(
        key,
        Key::Char(_) | Key::AltEnter | Key::ShiftEnter | Key::Backspace | Key::Delete
    )
}

fn reduce_keymap(
    state: &mut AppState,
    key: Key,
    cancel_controller: Option<&CancelController>,
) -> bool {
    let idle = state.phase == UiPhase::Idle;
    match state.input.mode {
        InputMode::Insert => reduce_insert_key(state, key, idle, cancel_controller),
        InputMode::Normal => reduce_normal_key(state, key, idle, cancel_controller),
        InputMode::Visual => reduce_visual_key(state, key, idle, cancel_controller),
    }
}

/// Insert mode: j/k exit chords (the second key within the window leaves insert
/// mode), Esc leaves insert mode, everything else falls through.
fn reduce_insert_key(
    state: &mut AppState,
    key: Key,
    idle: bool,
    cancel_controller: Option<&CancelController>,
) -> bool {
    match key {
        Key::Esc => reduce_insert_escape(state, idle, cancel_controller),
        Key::Char('j') | Key::Char('k') => reduce_insert_chord(state, key, idle),
        other => {
            state.input.clear_insert_exit_pending_j();
            state.input.clear_normal_pending_key();
            reduce_common_key(state, other, cancel_controller)
        }
    }
}

fn reduce_insert_escape(
    state: &mut AppState,
    idle: bool,
    cancel_controller: Option<&CancelController>,
) -> bool {
    state.input.clear_insert_exit_pending_j();
    state.input.clear_normal_pending_key();
    if idle {
        return handle_escape(state, cancel_controller);
    }
    // Busy: Esc drops to normal mode first — itself a repaint — then the escape
    // coupling arms the cancel confirmation.
    state.enter_normal_mode();
    let _ = handle_escape(state, cancel_controller);
    true
}

/// The insert-exit chord. Arming it is an input-state change, so a busy turn
/// repaints; while idle the armed key reports no change, exactly as the lens
/// did.
fn reduce_insert_chord(state: &mut AppState, key: Key, idle: bool) -> bool {
    let Key::Char(ch) = key else {
        return false;
    };
    if state.input.insert_exit_pending_j() {
        state.input.clear_insert_exit_pending_j();
        if idle {
            return handle_enter_normal_mode_from_chord(state);
        }
        state.enter_normal_mode();
        return true;
    }
    if ch == 'j' {
        state.input.set_insert_exit_pending_j();
    } else {
        state.input.clear_insert_exit_pending_j();
    }
    !idle
}

/// Normal mode: the vim navigation keymap, the pending `gg` key, and the mode
/// transitions. While busy the keymap still applies, but `i` re-enters insert
/// mode directly instead of through the idle gate.
fn reduce_normal_key(
    state: &mut AppState,
    key: Key,
    idle: bool,
    cancel_controller: Option<&CancelController>,
) -> bool {
    state.input.clear_insert_exit_pending_j();
    let select = false;
    match key {
        Key::Esc => {
            state.input.clear_normal_pending_key();
            handle_escape(state, cancel_controller)
        }
        Key::Char('i') => {
            state.input.clear_normal_pending_key();
            if idle {
                handle_enter_insert_mode(state)
            } else {
                state.enter_insert_mode();
                true
            }
        }
        Key::Char('v') => {
            state.input.clear_normal_pending_key();
            handle_enter_visual_mode(state)
        }
        Key::Char('j') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::LineDown { select })
        }
        Key::Char('k') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::LineUp { select })
        }
        Key::Char('h') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::FocusPaneLeft)
        }
        Key::Char('l') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::FocusPaneRight)
        }
        Key::Tab => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::FocusPaneRight)
        }
        Key::BackTab => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::FocusPaneLeft)
        }
        Key::Char('g') => reduce_pending_g(state),
        Key::Char('G') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::ToBottom { select })
        }
        Key::PageUp | Key::CtrlU => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::PageUp { select })
        }
        Key::PageDown | Key::CtrlD => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::PageDown { select })
        }
        Key::Char(_) | Key::AltEnter | Key::ShiftEnter => {
            state.input.clear_normal_pending_key();
            false
        }
        other => {
            state.input.clear_normal_pending_key();
            reduce_common_key(state, other, cancel_controller)
        }
    }
}

/// Visual mode selects as it moves. While idle it owns a closed keymap — a key
/// it does not name does nothing; while busy the keys pass through to the
/// common arm, exactly as the lens did.
fn reduce_visual_key(
    state: &mut AppState,
    key: Key,
    idle: bool,
    cancel_controller: Option<&CancelController>,
) -> bool {
    if !idle {
        state.input.clear_insert_exit_pending_j();
        state.input.clear_normal_pending_key();
        return reduce_common_key(state, key, cancel_controller);
    }
    let select = true;
    match key {
        Key::CtrlP => {
            state.input.clear_normal_pending_key();
            handle_toggle_command_palette(state)
        }
        Key::Char('j') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::LineDown { select })
        }
        Key::Char('k') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::LineUp { select })
        }
        Key::Char('g') => reduce_pending_g(state),
        Key::Char('G') => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::ToBottom { select })
        }
        Key::Char('y') => {
            state.input.clear_normal_pending_key();
            handle_yank_selection(state)
        }
        Key::Esc => {
            state.input.clear_normal_pending_key();
            handle_escape(state, cancel_controller)
        }
        Key::PageUp | Key::CtrlU => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::PageUp { select })
        }
        Key::PageDown | Key::CtrlD => {
            state.input.clear_normal_pending_key();
            state
                .scroll
                .reduce_scroll_action(ScrollAction::PageDown { select })
        }
        _ => {
            state.input.clear_normal_pending_key();
            false
        }
    }
}

/// The pending `gg` key: the first `g` arms it, the second one jumps to the top
/// of the transcript.
fn reduce_pending_g(state: &mut AppState) -> bool {
    let select = state.input.mode == InputMode::Visual;
    if state.input.take_normal_pending_key_if('g') {
        state
            .scroll
            .reduce_scroll_action(ScrollAction::ToTop { select })
    } else {
        state.input.arm_normal_pending_key('g');
        false
    }
}

/// The keys every mode shares: submit, quit, the palette, escape, and paging.
fn reduce_common_key(
    state: &mut AppState,
    key: Key,
    cancel_controller: Option<&CancelController>,
) -> bool {
    let select = state.input.mode == InputMode::Visual;
    match key {
        Key::Enter => {
            let text = state.input.pending_submit_text.take().unwrap_or_default();
            // The live submit boundary: the facts are stamped as the prompt is
            // accepted, and they travel with it to the row.
            let facts = state.submit_facts();
            handle_submit(state, text, facts)
        }
        Key::CtrlC => handle_quit(state, cancel_controller),
        Key::CtrlP => handle_toggle_command_palette(state),
        Key::Esc => handle_escape(state, cancel_controller),
        Key::PageUp | Key::CtrlU => state
            .scroll
            .reduce_scroll_action(ScrollAction::PageUp { select }),
        Key::PageDown | Key::CtrlD => state
            .scroll
            .reduce_scroll_action(ScrollAction::PageDown { select }),
        // Editing keys are the coordinator's TextArea; at the reducer boundary
        // they are no-ops. History and query cycling are coordinator-side too.
        _ => false,
    }
}

/// A wheel notch goes through the same modal gates as a key: an open info
/// panel or picker owns the notch (the inline slash list passes it through), and
/// the permission card swallows it. The gates report a repaint exactly as the
/// lens tuples did.
pub(super) fn reduce_wheel_event(state: &mut AppState, up: bool, column: u16, row: u16) -> bool {
    if state.info_panel.is_some() {
        return false;
    }
    if state.picker.active().is_some() && state.picker.active() != Some(ActivePicker::InlineSlash) {
        return true;
    }
    if state.permission.has_prompt() {
        return true;
    }
    reduce_wheel(state, up, column, row)
}

/// A wheel notch over the transcript leaves insert mode and scrolls. The
/// insert-to-normal transition mutates the rendered mode indicator and pane
/// focus, so it is a visible change even when the scroll is a no-op.
fn reduce_wheel(state: &mut AppState, up: bool, column: u16, row: u16) -> bool {
    let select = state.input.mode == InputMode::Visual;
    if !state.scroll.contains_transcript_point(column, row) {
        return false;
    }
    let mode_changed = state.input.mode == InputMode::Insert;
    if mode_changed {
        state.enter_normal_mode();
    }
    let action = if up {
        ScrollAction::WheelUp { select }
    } else {
        ScrollAction::WheelDown { select }
    };
    state.scroll.reduce_scroll_action(action) || mode_changed
}

// endregion: --- Key arms
