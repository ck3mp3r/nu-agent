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
}
