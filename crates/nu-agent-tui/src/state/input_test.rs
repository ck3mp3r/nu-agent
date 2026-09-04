//! Unit tests for [`InputState`] mode rewriting, chords, and history.

use super::*;
use crate::interaction::reducer::UserAction;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_input_state_handle_action_idle_normal_keymap_maps_vim_keys() {
    // -- Setup & Fixtures
    let mut input = InputState::default().with_mode(InputMode::Normal);

    // -- Exec & Check
    let (action, changed) = input.handle_action(UserAction::InsertChar('j'), true);
    assert_eq!(action, UserAction::ScrollLineDown);
    assert!(!changed);

    let (action, _) = input.handle_action(UserAction::InsertChar('k'), true);
    assert_eq!(action, UserAction::ScrollLineUp);

    let (action, _) = input.handle_action(UserAction::InsertChar('h'), true);
    assert_eq!(action, UserAction::FocusPaneLeft);

    let (action, _) = input.handle_action(UserAction::InsertChar('l'), true);
    assert_eq!(action, UserAction::FocusPaneRight);

    let (action, _) = input.handle_action(UserAction::CompleteForward, true);
    assert_eq!(action, UserAction::FocusPaneRight);

    let (action, _) = input.handle_action(UserAction::CompleteBackward, true);
    assert_eq!(action, UserAction::FocusPaneLeft);

    let (action, _) = input.handle_action(UserAction::InsertChar('i'), true);
    assert_eq!(action, UserAction::EnterInsertMode);

    let (action, _) = input.handle_action(UserAction::InsertChar('v'), true);
    assert_eq!(action, UserAction::EnterVisualMode);

    let (action, _) = input.handle_action(UserAction::InsertChar('G'), true);
    assert_eq!(action, UserAction::ScrollToBottom);

    let (action, _) = input.handle_action(UserAction::Esc, true);
    assert_eq!(action, UserAction::Esc);
}

#[test]
fn test_input_state_handle_action_idle_normal_unmapped_char_is_noop() {
    // -- Setup & Fixtures
    // Equivalent of the deleted reducer insert_char free fn: plain characters
    // in normal mode are consumed, not typed.
    let mut input = InputState::default().with_mode(InputMode::Normal);

    // -- Exec
    let (action, changed) = input.handle_action(UserAction::InsertChar('x'), true);

    // -- Check
    assert_eq!(action, UserAction::Noop);
    assert!(!changed);
}

#[test]
fn test_input_state_handle_action_idle_normal_gg_chord_arms_then_fires() {
    // -- Setup & Fixtures
    let mut input = InputState::default().with_mode(InputMode::Normal);

    // -- Exec & Check
    let (action, _) = input.handle_action(UserAction::InsertChar('g'), true);
    assert_eq!(action, UserAction::Noop);

    let (action, _) = input.handle_action(UserAction::InsertChar('g'), true);
    assert_eq!(action, UserAction::ScrollToTop);

    // The chord is consumed after firing; a third 'g' re-arms it.
    let (action, _) = input.handle_action(UserAction::InsertChar('g'), true);
    assert_eq!(action, UserAction::Noop);
}

#[test]
fn test_input_state_handle_action_idle_insert_j_chord_arms_then_fires() {
    // -- Setup & Fixtures
    let mut input = InputState::default();
    assert_eq!(input.mode, InputMode::Insert);

    // -- Exec & Check
    let (action, _) = input.handle_action(UserAction::InsertChar('j'), true);
    assert_eq!(action, UserAction::InsertChar('j'));

    let (action, _) = input.handle_action(UserAction::InsertChar('j'), true);
    assert_eq!(action, UserAction::EnterNormalModeFromChord);
    // Idle-mode rewriting does not transition the mode; the reducer does.
    assert_eq!(input.mode, InputMode::Insert);
}

#[test]
fn test_input_state_handle_action_idle_visual_y_maps_to_yank() {
    // -- Setup & Fixtures
    let mut input = InputState::default().with_mode(InputMode::Visual);

    // -- Exec
    let (action, _) = input.handle_action(UserAction::InsertChar('y'), true);

    // -- Check
    assert_eq!(action, UserAction::YankSelection);
}

#[test]
fn test_input_state_handle_action_busy_insert_esc_exits_to_normal_and_arms_chord() {
    // -- Setup & Fixtures
    let mut input = InputState::default();

    // -- Exec
    let (action, changed) = input.handle_action(UserAction::Esc, false);

    // -- Check
    assert_eq!(action, UserAction::Noop);
    assert!(changed);
    assert_eq!(input.mode, InputMode::Normal);
    assert!(input.insert_exit_pending_j());
}

#[test]
fn test_input_state_handle_action_busy_insert_jk_chord_transitions_to_normal() {
    // -- Setup & Fixtures
    let mut input = InputState::default();

    // -- Exec & Check
    let (action, changed) = input.handle_action(UserAction::InsertChar('j'), false);
    assert_eq!(action, UserAction::InsertChar('j'));
    assert!(changed);
    assert_eq!(input.mode, InputMode::Insert);

    let (action, changed) = input.handle_action(UserAction::InsertChar('j'), false);
    assert_eq!(action, UserAction::EnterNormalModeFromChord);
    assert!(changed);
    assert_eq!(input.mode, InputMode::Normal);

    // Re-entering insert mode resets the chord; the jk chord arms with 'j'
    // and fires with 'k' ('k' alone never arms — it only clears).
    input.enter_insert_mode();
    let (action, _) = input.handle_action(UserAction::InsertChar('j'), false);
    assert_eq!(action, UserAction::InsertChar('j'));

    let (action, changed) = input.handle_action(UserAction::InsertChar('k'), false);
    assert_eq!(action, UserAction::EnterNormalModeFromChord);
    assert!(changed);
    assert_eq!(input.mode, InputMode::Normal);
}

#[test]
fn test_input_state_handle_action_busy_normal_i_enters_insert() {
    // -- Setup & Fixtures
    let mut input = InputState::default().with_mode(InputMode::Normal);

    // -- Exec
    let (action, changed) = input.handle_action(UserAction::InsertChar('i'), false);

    // -- Check
    assert_eq!(action, UserAction::Noop);
    assert!(changed);
    assert_eq!(input.mode, InputMode::Insert);
}

#[test]
fn test_input_state_handle_action_busy_visual_resets_chords_and_passes_through() {
    // -- Setup & Fixtures
    let mut input = InputState::default().with_mode(InputMode::Visual);
    input.set_insert_exit_pending_j();

    // -- Exec
    let (action, changed) = input.handle_action(UserAction::ScrollLineDown, false);

    // -- Check
    assert_eq!(action, UserAction::ScrollLineDown);
    assert!(!changed);
    assert!(!input.insert_exit_pending_j());
}

#[test]
fn test_input_state_insert_exit_chord_expires_after_timeout() {
    // -- Setup & Fixtures
    let mut input = InputState::default();
    input.set_insert_exit_pending_j();

    // -- Exec
    std::thread::sleep(std::time::Duration::from_millis(600));

    // -- Check
    assert!(!input.insert_exit_pending_j());
}

#[test]
fn test_input_state_history_up_loads_newest_first_then_clamps_at_oldest() -> Result<()> {
    // -- Setup & Fixtures
    let mut input = InputState::default();
    let submitted = vec!["a".to_string(), "b".to_string(), "c".to_string()];

    // -- Exec & Check
    assert_eq!(
        input.history_up(&submitted, ""),
        Some("c".to_string()),
        "should load newest first"
    );
    assert_eq!(input.history_up(&submitted, ""), Some("b".to_string()));
    assert_eq!(input.history_up(&submitted, ""), Some("a".to_string()));
    assert_eq!(
        input.history_up(&submitted, ""),
        Some("a".to_string()),
        "should clamp at the oldest entry"
    );
    Ok(())
}

#[test]
fn test_input_state_history_down_past_newest_restores_saved_draft() -> Result<()> {
    // -- Setup & Fixtures
    let mut input = InputState::default();
    let submitted = vec!["p1".to_string()];

    // -- Exec
    let up = input
        .history_up(&submitted, "draft")
        .ok_or("should load history up")?;
    let down = input.history_down(&submitted);

    // -- Check
    assert_eq!(up, "p1");
    assert_eq!(down, Some("draft".to_string()));
    Ok(())
}

#[test]
fn test_input_state_history_with_empty_submitted_returns_none() {
    // -- Setup & Fixtures
    let mut input = InputState::default();
    let submitted: Vec<String> = Vec::new();

    // -- Exec & Check
    assert_eq!(input.history_up(&submitted, ""), None);
    assert_eq!(input.history_down(&submitted), None);
}

#[test]
fn test_input_state_reset_history_navigation_clears_index_and_saved_draft() {
    // -- Setup & Fixtures
    let mut input = InputState::default();
    let submitted = vec!["p1".to_string()];
    let _ = input.history_up(&submitted, "draft");

    // -- Exec
    input.reset_history_navigation();

    // -- Check
    assert_eq!(input.history_down(&submitted), None);
}

#[test]
fn test_input_state_clipboard_request_take_consumes_payload() {
    // -- Setup & Fixtures
    let mut input = InputState::default();
    input.set_clipboard_request("payload".to_string());

    // -- Exec & Check
    assert_eq!(input.take_clipboard_request(), Some("payload".to_string()));
    assert_eq!(input.take_clipboard_request(), None);
}
