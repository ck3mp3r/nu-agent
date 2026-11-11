//! The settle race: when the turn ends between the arming press and the
//! confirming one, the next idle Esc reports it once and changes nothing else.

use super::*;
use crate::test_support::{fold_record, turn_completed};

/// WHEN the turn settles between the arming press and the confirming press, THE
/// next idle Esc SHALL show the settled notice and SHALL leave the input mode
/// alone.
#[test]
fn test_interaction_abort_cancel_settle_race_shows_finished_notice() -> Result<()> {
    // -- Setup & Fixtures: a running turn, armed by one Esc.
    let (mut state, cancel_controller) = busy_state_with_controller();
    let _ = state.activate_next_prompt();
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert_eq!(state.input.mode, InputMode::Normal);

    // -- Exec: the turn settles before the confirming press.
    let settled = fold_record(&mut state, turn_completed(0));

    // -- Check: idle, unarmed, and the arm's status is gone.
    assert!(settled);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);

    // -- Exec: the Esc that would have confirmed lands on an idle surface.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the notice, no cancel, and the mode is where it was.
    assert!(changed, "the notice must repaint");
    assert_eq!(state.status.message.status_line(), ESC_SETTLED_STATUS);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert!(!cancel_controller.is_cancel_requested());

    // -- Setup & Fixtures: the same race, with the user back in insert mode.
    let (mut state, cancel_controller) = busy_state_with_controller();
    let _ = state.activate_next_prompt();
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    fold_record(&mut state, turn_completed(0));
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('i')),
        Some(&cancel_controller),
    );
    assert_eq!(state.input.mode, InputMode::Insert);

    // -- Exec: the idle Esc arrives in insert mode.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the notice replaces the silent mode toggle.
    assert!(changed);
    assert_eq!(state.status.message.status_line(), ESC_SETTLED_STATUS);
    assert_eq!(
        state.input.mode,
        InputMode::Insert,
        "the settled notice must not change the input mode"
    );
    assert!(!cancel_controller.is_cancel_requested());
    Ok(())
}

/// WHEN one Esc displays the settled notice, THE outstanding flag is consumed:
/// the next Esc behaves per the idle criterion and the notice does not repeat.
#[test]
fn test_interaction_abort_cancel_settle_notice_is_consumed_once() -> Result<()> {
    // -- Setup & Fixtures: arm, settle, back to insert mode, one Esc for the
    // notice.
    let (mut state, cancel_controller) = busy_state_with_controller();
    let _ = state.activate_next_prompt();
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    fold_record(&mut state, turn_completed(0));
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('i')),
        Some(&cancel_controller),
    );
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert_eq!(state.status.message.status_line(), ESC_SETTLED_STATUS);

    // -- Exec: the next Esc.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the idle insert behavior — the mode exits, no second notice.
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Idle);

    // -- Exec: one more Esc in idle normal mode.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the notice is spent, so nothing is left to report.
    assert!(
        !changed,
        "a spent notice must not resurface on an idle normal Esc"
    );
    assert!(!cancel_controller.is_cancel_requested());
    Ok(())
}
