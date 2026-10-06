use super::*;

#[test]
fn inline_slash_suggestions_open_on_leading_slash() {
    let mut state = AppState::default();

    // check_inline_slash is called by the coordinator after TextArea mutations.
    // In the dispatch-only test, call it directly to set the state.
    state.check_inline_slash("/");

    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
}

#[test]
fn inline_slash_enter_on_compact_triggers_compaction_path() {
    let mut state = AppState {
        input: InputState::default().with_pending_submit_text("/compact".to_string()),
        ..Default::default()
    };
    // InsertChar is now a no-op in the dispatch path (handled by TextArea).
    // Set pending_submit_text directly so Submit routes the slash command.

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(changed);
    assert!(state.transcript.blocks().is_empty());
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
}

#[test]
fn immediate_slash_commands_do_not_set_busy_or_spinner() {
    for command in ["/compact", "/mcp", "/help", "/status", "/skills"] {
        let mut state = AppState {
            input: InputState::default().with_pending_submit_text(command.to_string()),
            ..Default::default()
        };

        // InsertChar is now a no-op in the dispatch path (handled by TextArea).
        // Set pending_submit_text directly so Submit routes the slash command.

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

#[test]
fn inline_slash_suggestions_close_when_prefix_removed() {
    let mut state = AppState::default();
    state.check_inline_slash("/");
    assert_eq!(
        state.picker.render_kind(),
        Some(PickerRenderKind::InlineSlash)
    );

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
