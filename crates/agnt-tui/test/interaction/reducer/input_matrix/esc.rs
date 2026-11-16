//! Rows 18-21: the Esc trio — arm, confirm, expiry, and the confirm's end state.

use super::*;

// row 18 — one busy Esc exits insert mode and arms the abort.
#[test]
fn test_input_matrix_locked_escape_arms_the_abort() -> Result<()> {
    // -- Setup & Fixtures: a running turn in insert mode.
    let mut state = busy_state_with_clean_transcript();

    // -- Exec: the one press that is two things.
    let changed = fold_key(&mut state, Key::Esc, None);

    // -- Check: the mode exits and the arm carries its five-second window.
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.scroll.pane_focus, PaneFocus::Transcript);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert!(state.abort.armed_at.is_some());
    assert_eq!(state.abort.arm_window, Duration::from_secs(5));
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    Ok(())
}

// row 19 — the confirming Esc inside the window cancels and restores.
#[test]
fn test_input_matrix_confirming_escape_cancels_and_restores() -> Result<()> {
    // -- Setup & Fixtures: a running turn with one prompt queued behind it.
    let mut state = busy_state_with_clean_transcript();
    state.input.pending_submit_text = Some("queued".to_string());
    fold_key(&mut state, Key::Enter, None);
    let controller = CancelController::default();
    fold_key(&mut state, Key::Esc, Some(&controller));
    assert_eq!(state.phase, UiPhase::AbortPending);

    // -- Exec: the confirming press lands inside the window.
    let changed = fold_key(&mut state, Key::Esc, Some(&controller));

    // -- Check: the cancel fired and the queued text came back to the draft.
    assert!(changed);
    assert!(controller.is_cancel_requested());
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.input_locked);
    assert_eq!(state.input.restored_input_text.as_deref(), Some("queued"));
    assert!(state.status.message.status_line().is_empty());
    Ok(())
}

// row 20 — an expired arm drops silently and the next Esc re-arms.
#[test]
fn test_input_matrix_expired_arm_rearms_instead_of_cancelling() -> Result<()> {
    // -- Setup & Fixtures: a running turn, armed by one Esc.
    let mut state = busy_state_with_clean_transcript();
    fold_key(&mut state, Key::Esc, None);
    assert_eq!(state.phase, UiPhase::AbortPending);
    let armed_at = state.abort.armed_at.ok_or("the arm should be stamped")?;
    let expired_at = armed_at + state.abort.arm_window + Duration::from_millis(1);

    // -- Exec: the clock sweeps the window out.
    let swept = state.sweep_expired_abort(expired_at);

    // -- Check: no notice is owed, the turn is busy again, the mode holds.
    assert!(swept);
    assert!(!state.abort.pending);
    assert!(!state.abort.settle_notice);
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Normal);

    // -- Exec: the next Esc on the still-running turn.
    let controller = CancelController::default();
    let changed = fold_key(&mut state, Key::Esc, Some(&controller));

    // -- Check: it re-arms, the message returns, nothing cancels or restores.
    assert!(changed);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    assert!(!controller.is_cancel_requested());
    assert_eq!(state.input.restored_input_text, None);
    Ok(())
}

// row 21 — a confirmed cancel lands in insert mode, and the release leaves the
// mode alone.
#[test]
fn test_input_matrix_confirmed_cancel_lands_in_insert() -> Result<()> {
    // -- Setup & Fixtures: a running turn with a queued prompt and a controller.
    let mut state = busy_state_with_clean_transcript();
    state.input.pending_submit_text = Some("queued".to_string());
    fold_key(&mut state, Key::Enter, None);
    let controller = CancelController::default();
    fold_key(&mut state, Key::Esc, Some(&controller));

    // -- Exec: the confirming press completes the cancel.
    fold_key(&mut state, Key::Esc, Some(&controller));

    // -- Check: insert mode, unlocked, with the restored text waiting.
    assert_eq!(state.input.mode, InputMode::Insert);
    assert_eq!(state.scroll.pane_focus, PaneFocus::Input);
    assert!(!state.input_locked);
    assert_eq!(state.input.restored_input_text.as_deref(), Some("queued"));

    // -- Setup & Fixtures: a running turn released while the surface sits in
    // normal mode.
    let mut released = busy_state_with_clean_transcript();
    released.enter_normal_mode();

    // -- Exec: the spine's turn-end step.
    fold_turn_completed(&mut released);

    // -- Check: the release drops the lock and never touches the mode.
    assert!(!released.input_locked);
    assert_eq!(released.input.mode, InputMode::Normal);
    Ok(())
}
