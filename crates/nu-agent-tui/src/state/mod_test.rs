use crate::state::*;

#[test]
fn defaults_start_idle_with_unlocked_input_and_no_abort_pending() {
    let state = AppState::default();

    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert!(!state.abort.pending);
    assert_eq!(state.abort.confirmation_marker, 0);
    assert!(state.transcript_preview.is_empty());
    assert!(state.status_line.is_empty());
    assert_eq!(state.latest_input_tokens, None);
    assert_eq!(state.latest_output_tokens, None);
    assert_eq!(state.latest_total_tokens, None);
    assert_eq!(state.session_total_tokens, 0);
}

#[test]
fn mode_helpers_toggle_between_insert_and_normal() {
    let mut state = AppState::default();
    assert_eq!(state.input_mode, InputMode::Insert);

    state.enter_normal_mode();
    assert_eq!(state.input_mode, InputMode::Normal);

    state.enter_insert_mode();
    assert_eq!(state.input_mode, InputMode::Insert);
}

#[test]
fn normal_mode_defaults_focus_to_transcript_and_insert_focuses_input() {
    let mut state = AppState::default();
    assert_eq!(state.pane_focus, PaneFocus::Input);

    state.enter_normal_mode();
    assert_eq!(state.pane_focus, PaneFocus::Transcript);

    state.enter_insert_mode();
    assert_eq!(state.pane_focus, PaneFocus::Input);
}

#[test]
fn pane_focus_can_cycle_left_and_right() {
    let mut state = AppState::default();
    state.enter_normal_mode();

    state.focus_next_pane();
    assert_eq!(state.pane_focus, PaneFocus::Input);

    state.focus_next_pane();
    assert_eq!(state.pane_focus, PaneFocus::Transcript);

    state.focus_prev_pane();
    assert_eq!(state.pane_focus, PaneFocus::Input);
}
