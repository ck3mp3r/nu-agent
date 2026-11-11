//! The expired arm: a press after the message lifetime disarms and re-arms
//! instead of cancelling; a re-stamped live arm executes.

use super::*;

/// WHEN the armed abort's message lifetime is over, THE press SHALL drop the
/// arm silently and SHALL re-arm instead of cancelling. A re-stamped deadline
/// inside the window SHALL execute.
#[test]
fn test_interaction_abort_cancel_expired_arm_rearms_on_press() -> Result<()> {
    // -- Setup & Fixtures: a running turn whose arm window is already over.
    let (mut state, cancel_controller) = busy_state_with_controller();
    state.abort.arm_window = std::time::Duration::ZERO;
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert!(!state.abort.arm_live(), "the window is over");

    // -- Exec: the press that would have confirmed lands after the window.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: no cancel out of nowhere; the press disarmed and re-armed.
    assert!(changed, "the re-arm must repaint");
    assert!(
        !cancel_controller.is_cancel_requested(),
        "an expired arm must not cancel"
    );
    assert_eq!(state.phase, UiPhase::AbortPending, "the press re-arms");
    assert!(state.abort.pending, "the press re-arms");
    assert_eq!(
        state.status.message.status_line(),
        ESC_ABORT_CONFIRM_STATUS,
        "the message reappears"
    );

    // -- Setup & Fixtures: the re-stamped deadline is live again.
    state.abort.arm_window = std::time::Duration::from_secs(5);

    // -- Exec: the next bare Esc.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the live arm executes.
    assert!(changed, "the confirm must repaint");
    assert!(
        cancel_controller.is_cancel_requested(),
        "a live arm cancels"
    );
    Ok(())
}
