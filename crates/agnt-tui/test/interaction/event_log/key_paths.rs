//! The key-path scenarios of the event-log suite.
//!
//! A key path folds through the same `reduce` entry point the records take, so
//! the fixtures and the imports come from the parent module.

use super::*;

// region:    --- Key-path scenarios

/// The insert-exit chord: one `j` arms the chord, the second one leaves
/// insert mode. The same fold must hold when the keymap lives in the reducer
/// instead of the input lens.
#[test]
fn golden_key_insert_exit_chord_reaches_normal_mode() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert, "starts in insert mode");

    // -- Exec: the first `j` only arms the chord.
    let first = reduce(
        &mut state,
        UiInput::KeyPressed {
            key: Key::Char('j'),
        },
        None,
    );

    // -- Check: arming the chord edits nothing.
    assert!(!first, "the armed chord reports no change");
    assert_eq!(state.input.mode, InputMode::Insert);

    // -- Exec: the second `j` is the chord.
    let second = reduce(
        &mut state,
        UiInput::KeyPressed {
            key: Key::Char('j'),
        },
        None,
    );

    // -- Check
    assert!(second, "the chord transition is a visible change");
    assert_eq!(state.input.mode, InputMode::Normal);
    Ok(())
}

/// Normal-mode navigation: `g` arms the pending key and the second `g` jumps
/// to the top of the transcript.
#[test]
fn golden_key_normal_pending_g_scrolls_to_top() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState {
        input: crate::state::InputState::default().with_mode(InputMode::Normal),
        ..Default::default()
    };
    state.scroll.viewport_height = 10;
    state.scroll.cursor_visual_row = 5;
    state.scroll.total_visual_rows = 30;

    // -- Exec: the first `g` only arms the pending key.
    let first = reduce(
        &mut state,
        UiInput::KeyPressed {
            key: Key::Char('g'),
        },
        None,
    );

    // -- Check: arming the pending key reports no change.
    assert!(!first, "the armed pending key reports no change");
    assert_eq!(state.scroll.cursor_visual_row, 5);

    // -- Exec: the second `g` scrolls to the top.
    let second = reduce(
        &mut state,
        UiInput::KeyPressed {
            key: Key::Char('g'),
        },
        None,
    );

    // -- Check
    assert!(second);
    assert_eq!(state.scroll.cursor_visual_row, 0);
    Ok(())
}

/// A busy-mode Esc leaves insert mode and arms the turn-cancel confirmation.
#[test]
fn golden_key_busy_escape_arms_abort_confirmation() -> Result<()> {
    // -- Setup & Fixtures: a running turn, input locked. The cycle starts at
    // the surface's own submit lane — the record fold never submits.
    let mut state = AppState::default();
    state.enqueue_prompt("say hi".to_string());
    let _ = state.activate_next_prompt();
    reduce(&mut state, RenderInput::Record(&inference_started()), None);
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);

    // -- Exec
    let changed = reduce(&mut state, UiInput::KeyPressed { key: Key::Esc }, None);

    // -- Check: mode left insert and the abort confirmation is armed.
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert_eq!(
        state.status.message.status_line(),
        crate::interaction::reducer::ESC_ABORT_CONFIRM_STATUS
    );
    Ok(())
}

/// The permission card owns its decision keys: `a` writes exactly one
/// AllowOnce decision through the key path.
#[test]
fn golden_key_permission_card_decision() -> Result<()> {
    // -- Setup & Fixtures
    let cancel_controller = CancelController::default();
    let arguments = r#"{"path":"a.rs"}"#;
    let call = edit_call(arguments);
    let mut state = AppState::default();
    fold_datum(
        &mut state,
        &record(tool_started(call.clone())),
        &cancel_controller,
    );
    fold_datum(
        &mut state,
        &permission_requested(&call, None),
        &cancel_controller,
    );
    assert!(state.permission.has_prompt(), "the card is up");

    // -- Exec
    let changed = reduce(
        &mut state,
        UiInput::KeyPressed {
            key: Key::Char('a'),
        },
        Some(&cancel_controller),
    );
    let effects = state.take_pending_events(&cancel_controller);

    // -- Check
    assert!(changed);
    assert!(
        !state.permission.has_prompt(),
        "the card closes on decision"
    );
    assert_eq!(effects.len(), 1, "exactly one permission decision");
    assert!(matches!(
        effects.first(),
        Some(OrchestratorEvent::PermissionDecision { decision })
            if decision.decision == PermissionDecision::AllowOnce
                && decision.request_id == "ask-1"
    ));
    Ok(())
}

// endregion: --- Key-path scenarios
