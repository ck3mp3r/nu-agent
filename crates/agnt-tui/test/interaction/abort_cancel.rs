use super::*;
use crate::test_support::entry_text;

#[test]
fn busy_escape_abort_table() -> Result<()> {
    // Esc in busy insert mode switches to normal and arms abort confirmation.
    let (mut state, cancel_controller) = busy_state_with_controller();
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    assert!(!cancel_controller.is_cancel_requested());

    // A second Esc from busy insert aborts.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert!(changed);
    assert!(cancel_controller.is_cancel_requested());
    assert!(state.status.message.status_line().is_empty());

    // The first escape in busy normal sets abort pending with the exact status text.
    let (mut state, cancel_controller) = busy_state_with_controller();
    state.enter_normal_mode();
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert!(changed);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    assert!(!cancel_controller.is_cancel_requested());

    // The second escape in abort pending after busy normal toggles cancel requested.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert!(changed);
    assert!(cancel_controller.is_cancel_requested());
    assert_eq!(state.input.mode, InputMode::Insert);
    assert!(state.status.message.status_line().is_empty());
    Ok(())
}

#[test]
fn test_interaction_abort_cancel_esc_with_card_arms_then_confirms() -> Result<()> {
    // -- Setup & Fixtures: a busy turn in normal mode with a permission card up.
    let (mut state, cancel_controller) = busy_state_with_controller();
    state.enter_normal_mode();
    open_permission_prompt(&mut state);
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Normal);

    // -- Exec: the first Esc arms the abort confirmation instead of denying.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: armed, no decision, card still up.
    assert!(changed);
    assert_eq!(state.phase, UiPhase::AbortPending);
    assert!(state.abort.pending);
    assert_eq!(state.status.message.status_line(), ESC_ABORT_CONFIRM_STATUS);
    assert!(!cancel_controller.is_cancel_requested());
    assert!(
        state.permission.has_prompt(),
        "the card stays up while the abort is armed"
    );
    assert!(
        state.permission.take_next_submission().is_none(),
        "arming the abort must not submit a permission decision"
    );

    // -- Exec: the second Esc confirms the cancel.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the confirm reduction ran.
    assert!(changed);
    assert!(cancel_controller.is_cancel_requested());
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert!(state.status.message.status_line().is_empty());
    assert!(
        !state.permission.has_prompt(),
        "confirming the cancel must close the card immediately"
    );
    assert!(
        state.permission.take_next_submission().is_none(),
        "confirming the cancel must not submit a permission decision"
    );
    Ok(())
}

#[test]
fn idle_escape_table() {
    // Escape in idle does not request cancellation.
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );
    assert!(changed);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert!(!cancel_controller.is_cancel_requested());
    assert_eq!(state.input.mode, InputMode::Normal);

    // Idle normal Esc returns false.
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        ..Default::default()
    };
    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(!changed);
    assert!(!state.abort.pending);
}

#[test]
fn typing_remains_available_while_prompt_is_active() {
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();

    state.input.pending_submit_text = Some("f".to_string());
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Enter),
        Some(&cancel_controller),
    );
    // Activate the prompt so the transcript entry is written
    let _ = state.take_next_prompt_for_execution();

    // Typing in insert mode is handled by the coordinator, not the dispatch path.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('s')),
        Some(&cancel_controller),
    );

    assert!(!changed);
    assert_eq!(state.transcript.len(), 1);
    assert_eq!(entry_text(&state.transcript.entries()[0]), "f");
}

/// WHEN a picker is open and a turn runs, one Esc closes the picker and ends
/// there — for every picker kind: no arm, no message, no deadline. The next
/// bare Esc arms, and the one after that confirms the cancel.
#[test]
fn test_interaction_abort_cancel_picker_esc_closes_only_while_busy() -> Result<()> {
    // -- Setup & Fixtures: one running turn per picker kind.
    for kind in [
        ActivePicker::CommandPalette,
        ActivePicker::Model,
        ActivePicker::Agent,
        ActivePicker::Session,
        ActivePicker::Theme,
        ActivePicker::InlineSlash,
    ] {
        let (mut state, cancel_controller) = busy_state_with_controller();
        if kind == ActivePicker::InlineSlash {
            state.check_inline_slash("/");
        } else {
            state.picker.open(kind);
        }
        assert_eq!(state.picker.active(), Some(kind));
        assert_eq!(state.phase, UiPhase::Busy);
        assert_eq!(state.input.mode, InputMode::Insert);

        // -- Exec: one Esc with the picker up and the turn running.
        let changed = dispatch_terminal_event(
            &mut state,
            &TerminalEvent::Key(TerminalKey::Esc),
            Some(&cancel_controller),
        );

        // -- Check: the picker closed and the press ended at the surface.
        assert!(changed, "{kind:?}: the press must repaint");
        assert_eq!(state.picker.active(), None, "{kind:?}: the picker closes");
        assert_eq!(state.phase, UiPhase::Busy, "{kind:?}: closing never arms");
        assert!(!state.abort.pending, "{kind:?}: closing never arms");
        assert!(
            state.abort.armed_at.is_none(),
            "{kind:?}: closing stamps no deadline"
        );
        assert_eq!(
            state.input.mode,
            InputMode::Insert,
            "{kind:?}: closing a surface leaves the mode alone"
        );
        assert!(
            state.status.message.status_line().is_empty(),
            "{kind:?}: closing shows no arm message"
        );
        assert!(
            !state.abort.settle_notice,
            "{kind:?}: closing owes no settled notice"
        );
        assert!(
            !cancel_controller.is_cancel_requested(),
            "{kind:?}: closing must not cancel"
        );

        // -- Exec: the second Esc is bare and arms the abort.
        let changed = dispatch_terminal_event(
            &mut state,
            &TerminalEvent::Key(TerminalKey::Esc),
            Some(&cancel_controller),
        );

        // -- Check: armed with the message showing, still no cancel.
        assert!(changed, "{kind:?}: the arm must repaint");
        assert_eq!(state.phase, UiPhase::AbortPending, "{kind:?}: armed");
        assert!(state.abort.pending, "{kind:?}: armed");
        assert!(
            state.abort.armed_at.is_some(),
            "{kind:?}: the arm stamps a deadline"
        );
        assert_eq!(
            state.status.message.status_line(),
            ESC_ABORT_CONFIRM_STATUS,
            "{kind:?}: the arm is visible"
        );
        assert!(
            !cancel_controller.is_cancel_requested(),
            "{kind:?}: arming must not cancel"
        );

        // -- Exec: the third Esc confirms the armed abort.
        let changed = dispatch_terminal_event(
            &mut state,
            &TerminalEvent::Key(TerminalKey::Esc),
            Some(&cancel_controller),
        );

        // -- Check: the double-press requirement held across the surface close.
        assert!(changed, "{kind:?}: the confirm must repaint");
        assert!(
            cancel_controller.is_cancel_requested(),
            "{kind:?}: the third press cancels"
        );
    }
    Ok(())
}

/// WHEN the info panel is open and a turn runs, one Esc closes the panel and
/// ends there — for both panel kinds. The next bare Esc arms, and the one
/// after that confirms the cancel.
#[test]
fn test_interaction_abort_cancel_panel_esc_closes_only() -> Result<()> {
    // -- Setup & Fixtures: one running turn per panel kind.
    for panel in [InfoPanel::Help, InfoPanel::Mcps] {
        let (mut state, cancel_controller) = busy_state_with_controller();
        state.open_info_panel(panel);
        assert_eq!(state.info_panel, Some(panel));
        assert_eq!(state.phase, UiPhase::Busy);
        assert_eq!(state.input.mode, InputMode::Insert);

        // -- Exec: one Esc with the panel up and the turn running.
        let changed = dispatch_terminal_event(
            &mut state,
            &TerminalEvent::Key(TerminalKey::Esc),
            Some(&cancel_controller),
        );

        // -- Check: the panel closed and the press ended at the surface.
        assert!(changed, "{panel:?}: the press must repaint");
        assert_eq!(state.info_panel, None, "{panel:?}: the panel closes");
        assert_eq!(state.phase, UiPhase::Busy, "{panel:?}: closing never arms");
        assert!(!state.abort.pending, "{panel:?}: closing never arms");
        assert!(
            state.abort.armed_at.is_none(),
            "{panel:?}: closing stamps no deadline"
        );
        assert_eq!(
            state.input.mode,
            InputMode::Insert,
            "{panel:?}: closing a surface leaves the mode alone"
        );
        assert!(
            state.status.message.status_line().is_empty(),
            "{panel:?}: closing shows no arm message"
        );
        assert!(
            !state.abort.settle_notice,
            "{panel:?}: closing owes no settled notice"
        );
        assert!(
            !cancel_controller.is_cancel_requested(),
            "{panel:?}: closing must not cancel"
        );

        // -- Exec: the second Esc is bare and arms the abort.
        let changed = dispatch_terminal_event(
            &mut state,
            &TerminalEvent::Key(TerminalKey::Esc),
            Some(&cancel_controller),
        );

        // -- Check: armed with the message showing, still no cancel.
        assert!(changed, "{panel:?}: the arm must repaint");
        assert_eq!(state.phase, UiPhase::AbortPending, "{panel:?}: armed");
        assert!(state.abort.pending, "{panel:?}: armed");
        assert_eq!(
            state.status.message.status_line(),
            ESC_ABORT_CONFIRM_STATUS,
            "{panel:?}: the arm is visible"
        );
        assert!(
            !cancel_controller.is_cancel_requested(),
            "{panel:?}: arming must not cancel"
        );

        // -- Exec: the third Esc confirms the armed abort.
        let changed = dispatch_terminal_event(
            &mut state,
            &TerminalEvent::Key(TerminalKey::Esc),
            Some(&cancel_controller),
        );

        // -- Check: the confirm reduction ran.
        assert!(changed, "{panel:?}: the confirm must repaint");
        assert!(
            cancel_controller.is_cancel_requested(),
            "{panel:?}: the third press cancels"
        );
    }
    Ok(())
}

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
    let settled = reduce(
        &mut state,
        SystemEvent::TurnCompleted {
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        },
        Some(&cancel_controller),
    );

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
    reduce(
        &mut state,
        SystemEvent::TurnCompleted {
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        },
        Some(&cancel_controller),
    );
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
    reduce(
        &mut state,
        SystemEvent::TurnCompleted {
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: None,
            cost: None,
            cost_complete: true,
            duration_ms: 0,
        },
        Some(&cancel_controller),
    );
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

/// WHEN no abort is outstanding and the turn is idle, Esc behavior is unchanged.
#[test]
fn test_interaction_abort_cancel_idle_esc_unchanged() -> Result<()> {
    // -- Setup & Fixtures: idle insert mode, nothing armed.
    let mut state = AppState::default();
    let cancel_controller = CancelController::default();
    assert_eq!(state.input.mode, InputMode::Insert);

    // -- Exec
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the plain mode exit, no notice, no arm, no cancel.
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Idle);
    assert!(!state.abort.pending);
    assert!(
        state.status.message.status_line().is_empty(),
        "an idle Esc has nothing to report"
    );
    assert!(!cancel_controller.is_cancel_requested());
    Ok(())
}
