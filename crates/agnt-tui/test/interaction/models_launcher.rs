use super::*;

#[test]
fn models_palette_and_slash_share_action_handler() {
    // The command palette Models action opens the inline model picker.
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    for _ in 0..4 {
        let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    }
    let selected = state.picker.active_state().unwrap().selected().unwrap();
    assert_eq!(selected.id, "Models");
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert_eq!(
        state.take_next_launch_request(),
        Some(SharedUiAction::Models)
    );
    assert_eq!(state.take_next_prompt_for_execution(), None);
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );

    // /models slash and the palette share the same action handler.
    let mut slash_state = AppState {
        input: InputState::default().with_pending_submit_text("/models".to_string()),
        ..Default::default()
    };
    let _ = dispatch_terminal_event(
        &mut slash_state,
        &TerminalEvent::Key(TerminalKey::Enter),
        None,
    );
    assert_eq!(
        slash_state.take_next_launch_request(),
        Some(SharedUiAction::Models)
    );
    assert_eq!(slash_state.take_next_prompt_for_execution(), None);

    let mut palette_state = AppState::default();
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::CtrlP),
        None,
    );
    for _ in 0..4 {
        let _ = dispatch_terminal_event(
            &mut palette_state,
            &TerminalEvent::Key(TerminalKey::Down),
            None,
        );
    }
    let _ = dispatch_terminal_event(
        &mut palette_state,
        &TerminalEvent::Key(TerminalKey::Enter),
        None,
    );
    assert_eq!(
        palette_state.take_next_launch_request(),
        Some(SharedUiAction::Models)
    );
    assert_eq!(palette_state.take_next_prompt_for_execution(), None);

    // The palette Models action does not bypass the shared models action path.
    let mut state = AppState::default();
    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    for _ in 0..4 {
        let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    }
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(changed);
    assert_ne!(state.picker.render_kind(), Some(PickerRenderKind::Model));
    assert_ne!(
        state.picker.render_kind(),
        Some(PickerRenderKind::CommandPalette)
    );
    assert_eq!(
        state.take_next_launch_request(),
        Some(SharedUiAction::Models)
    );
    assert_eq!(state.take_next_prompt_for_execution(), None);
}

#[test]
fn models_launcher_opens_picker_while_worker_active() {
    // Palette path.
    let mut state = AppState::default();
    state.enqueue_prompt("first".to_string());
    assert_eq!(
        state.take_next_prompt_for_execution(),
        Some("first".to_string())
    );

    let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::CtrlP), None);
    for _ in 0..4 {
        let _ = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Down), None);
    }
    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(changed);
    assert_eq!(
        state.take_next_launch_request(),
        Some(SharedUiAction::Models)
    );
    assert_eq!(state.take_next_prompt_for_execution(), None);
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(state.is_active_cycle());

    // Slash path.
    let mut state = AppState::default();
    state.enqueue_prompt("first".to_string());
    assert_eq!(
        state.take_next_prompt_for_execution(),
        Some("first".to_string())
    );
    state.input.pending_submit_text = Some("/models".to_string());

    let changed =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Enter), None);
    assert!(changed);
    assert_eq!(
        state.take_next_launch_request(),
        Some(SharedUiAction::Models)
    );
    assert_eq!(state.take_next_prompt_for_execution(), None);
    assert_eq!(state.phase, UiPhase::Busy);
    assert!(state.is_active_cycle());
}
