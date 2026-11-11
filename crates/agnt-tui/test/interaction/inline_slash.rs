use super::*;

#[test]
fn inline_slash_suggestions_open_and_close_table() {
    // Suggestions open on a leading slash.
    let mut state = AppState::default();
    state.check_inline_slash("/");
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    // Suggestions close when the prefix is removed.
    state.check_inline_slash("");
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn inline_slash_command_routing_table() {
    // Enter on /compact triggers the compaction path.
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("/compact".to_string()),
        ..Default::default()
    };
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(changed);
    assert!(state.transcript.entries().is_empty());
    assert_eq!(
        state.take_next_prompt_for_execution(),
        Some("/compact".to_string())
    );
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    // Immediate slash commands do not set busy or the spinner.
    for command in ["/compact", "/mcp", "/help", "/status", "/skills"] {
        let mut state = AppState {
            input: InputState::default().with_pending_submit_text(command.to_string()),
            ..Default::default()
        };

        let changed =
            dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
        assert!(changed);
        assert_eq!(state.phase, UiPhase::Idle);
        assert!(!state.is_active_cycle());
        assert_eq!(state.pending_prompt_count(), 0);
        assert!(state.prompt_items().is_empty());
        assert!(state.status.message.status_line() != "Thinking...");
    }

    // Tab on a real inline-slash selection submits the highlighted command.
    let mut state = AppState::default();
    state.check_inline_slash("/");
    for _ in 0..3 {
        if let Some(s) = state.picker.active_state_mut() {
            s.move_down();
        }
    }
    assert_eq!(
        state.picker.selected_slash_label(),
        Some("/status".to_string())
    );
    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);
    assert!(changed);
    assert_eq!(
        state.take_next_prompt_for_execution(),
        Some("/status".to_string())
    );
    assert_eq!(state.picker.active(), None);
    assert!(state.transcript.entries().is_empty());
}

/// WHEN the slash list is open on a running turn, one Esc SHALL close the list
/// and SHALL end there: no arm, no message, the draft and the mode untouched,
/// no deadline stamped.
#[test]
fn test_interaction_inline_slash_esc_closes_only_while_busy() -> Result<()> {
    // -- Setup & Fixtures: a running turn with the slash list open.
    let (mut state, cancel_controller) = busy_state_with_controller();
    state.check_inline_slash("/");
    assert_eq!(state.picker.active(), Some(ActivePicker::InlineSlash));
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);

    // -- Exec: one Esc.
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Esc),
        Some(&cancel_controller),
    );

    // -- Check: the list is gone and the press ended at the surface.
    assert!(changed);
    assert_eq!(state.picker.active(), None, "the list closes");
    assert_eq!(state.phase, UiPhase::Busy, "the press does not arm");
    assert!(!state.abort.pending, "the press does not arm");
    assert!(state.abort.armed_at.is_none(), "no deadline is stamped");
    assert_eq!(
        state.input.mode,
        InputMode::Insert,
        "the draft keeps its mode"
    );
    assert!(
        state.status.message.status_line().is_empty(),
        "no arm message"
    );
    assert!(!cancel_controller.is_cancel_requested());
    Ok(())
}
