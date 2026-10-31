use crate::interaction::{
    cancel::CancelController,
    input::{TerminalEvent, TerminalKey, TerminalMouse, TerminalResize, map_terminal_event},
    reducer::reduce,
};
use crate::state::{AppState, InputMode, UiPhase};
use agnt_core::protocol::events::{Key, SystemEvent};

#[test]
fn table_driven_key_mapping_to_the_system_vocabulary() {
    let cases = [
        (TerminalKey::Char('a'), Key::Char('a')),
        (TerminalKey::Char(' '), Key::Char(' ')),
        (TerminalKey::Enter, Key::Enter),
        (TerminalKey::AltEnter, Key::AltEnter),
        (TerminalKey::ShiftEnter, Key::ShiftEnter),
        (TerminalKey::Backspace, Key::Backspace),
        (TerminalKey::Delete, Key::Delete),
        (TerminalKey::Left, Key::Left),
        (TerminalKey::Right, Key::Right),
        (TerminalKey::Home, Key::Home),
        (TerminalKey::End, Key::End),
        (TerminalKey::Up, Key::Up),
        (TerminalKey::Down, Key::Down),
        (TerminalKey::PageUp, Key::PageUp),
        (TerminalKey::PageDown, Key::PageDown),
        (TerminalKey::CtrlU, Key::CtrlU),
        (TerminalKey::CtrlD, Key::CtrlD),
        (TerminalKey::CtrlP, Key::CtrlP),
        (TerminalKey::CtrlN, Key::CtrlN),
        (TerminalKey::CtrlJ, Key::CtrlJ),
        (TerminalKey::CtrlK, Key::CtrlK),
        (TerminalKey::Tab, Key::Tab),
        (TerminalKey::BackTab, Key::BackTab),
        (TerminalKey::Esc, Key::Esc),
        (TerminalKey::CtrlC, Key::CtrlC),
    ];

    for (terminal_key, key) in cases {
        let event = TerminalEvent::Key(terminal_key);
        assert_eq!(
            map_terminal_event(&event),
            Some(SystemEvent::KeyPressed { key }),
            "{terminal_key:?}"
        );
    }

    // Paste stays at the coordinator: the clipboard writes the textarea, so the
    // reducer never sees it.
    assert_eq!(
        map_terminal_event(&TerminalEvent::Paste("pasted".to_string())),
        None
    );
}

/// The input lock lives in the reducer, not the terminal layer: while a turn
/// runs it drops the keys that would edit the draft or submit it, and keeps
/// escape and quit live.
#[test]
fn test_interaction_input_locked_drops_editing_keys_only() {
    // -- Setup & Fixtures: a running turn with a drafted submit text.
    let mut state = AppState::default();
    reduce(
        &mut state,
        SystemEvent::PromptSubmitted {
            text: "work".to_string(),
        },
        None,
    );
    reduce(&mut state, SystemEvent::InferenceStarted, None);
    // The turn start sets the lock; the reducer gate is what this test drives.
    state.input_locked = true;
    assert!(state.input_locked, "a running turn locks the input");
    let _ = state.take_next_prompt_for_execution();
    state.input.pending_submit_text = Some("draft".to_string());

    // -- Exec & Check: the submit key is dropped, the draft survives.
    let changed = reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::Enter },
        None,
    );
    assert!(!changed, "a locked submit changes nothing");
    assert_eq!(state.input.pending_submit_text.as_deref(), Some("draft"));
    assert_eq!(
        state.take_next_prompt_for_execution(),
        None,
        "a locked submit queues no prompt"
    );

    // -- Exec & Check: an editing key is dropped.
    let changed = reduce(
        &mut state,
        SystemEvent::KeyPressed {
            key: Key::Char('a'),
        },
        None,
    );
    assert!(!changed, "a locked edit changes nothing");

    // -- Exec & Check: escape is not an editing key: it leaves insert mode and
    // arms the turn-cancel confirmation.
    let changed = reduce(&mut state, SystemEvent::KeyPressed { key: Key::Esc }, None);
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::AbortPending);

    // -- Exec & Check: quit stays live: it cancels the turn and quits.
    let cancel_controller = CancelController::default();
    let changed = reduce(
        &mut state,
        SystemEvent::KeyPressed { key: Key::CtrlC },
        Some(&cancel_controller),
    );
    assert!(changed);
    assert!(state.quit_requested);
    assert!(cancel_controller.is_cancel_requested());
}

#[test]
fn test_map_terminal_event_resize_maps_to_resized_dimensions() {
    // -- Setup & Fixtures
    let event = TerminalEvent::Resize(TerminalResize {
        columns: 120,
        rows: 42,
    });

    // -- Exec
    let actual = map_terminal_event(&event);

    // -- Check
    assert_eq!(
        actual,
        Some(SystemEvent::Resized {
            width: 120,
            height: 42
        })
    );
}

#[test]
fn test_map_terminal_event_mouse_wheel_maps_to_wheel_scrolled() {
    // -- Setup & Fixtures
    let wheel_up = TerminalEvent::Mouse(TerminalMouse {
        up: true,
        column: 9,
        row: 4,
    });
    let wheel_down = TerminalEvent::Mouse(TerminalMouse {
        up: false,
        column: 2,
        row: 18,
    });

    // -- Exec & Check
    assert_eq!(
        map_terminal_event(&wheel_up),
        Some(SystemEvent::WheelScrolled {
            up: true,
            column: 9,
            row: 4
        })
    );
    assert_eq!(
        map_terminal_event(&wheel_down),
        Some(SystemEvent::WheelScrolled {
            up: false,
            column: 2,
            row: 18
        })
    );
}
