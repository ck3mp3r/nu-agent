//! The close-only tables: with a picker or a panel open and a turn running, one
//! Esc closes the surface and ends there — no arm, no message, no deadline.

use super::*;

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
