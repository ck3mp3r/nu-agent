use super::*;

#[test]
fn insert_mode_chord_table() {
    // Esc in idle insert mode switches to normal mode.
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);
    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Idle);

    // jj / jk chords in busy insert mode switch to normal mode.
    for second in ['j', 'k'] {
        let mut state = busy_state();
        assert_eq!(state.phase, UiPhase::Busy);
        assert_eq!(state.input.mode, InputMode::Insert);
        let first = dispatch_terminal_event(
            &mut state,
            &TerminalEvent::Key(TerminalKey::Char('j')),
            None,
        );
        assert!(first);
        assert_eq!(state.input.mode, InputMode::Insert);
        let second_changed = dispatch_terminal_event(
            &mut state,
            &TerminalEvent::Key(TerminalKey::Char(second)),
            None,
        );
        assert!(second_changed);
        assert_eq!(state.input.mode, InputMode::Normal);
        assert_eq!(state.phase, UiPhase::Busy);
    }

    // The insert-mode jk chord enters normal mode (the first j is a no-op).
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);
    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(!first);
    assert_eq!(state.input.mode, InputMode::Insert);
    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(second);
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn test_interaction_mode_switching_esc_with_card_exits_insert() -> Result<()> {
    // -- Setup & Fixtures: a busy turn in insert mode with a permission card up.
    let mut state = busy_state();
    open_permission_prompt(&mut state);
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);

    // -- Exec
    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);

    // -- Check: the mode exits, the card stays up, no decision is submitted.
    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert!(
        state.permission.has_prompt(),
        "Esc must leave the card open"
    );
    assert!(
        state.permission.take_next_submission().is_none(),
        "Esc must not submit a permission decision"
    );
    Ok(())
}

#[test]
fn normal_mode_focus_cycle_table() {
    // h/l cycle focus between panes.
    let mut state = AppState::default();
    state.enter_normal_mode();
    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('l')),
        None,
    );
    assert!(first);
    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Input);
    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('h')),
        None,
    );
    assert!(second);
    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Transcript);

    // Tab/BackTab cycle focus between transcript and input.
    let mut state = AppState::default();
    state.enter_normal_mode();
    let tab = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Tab), None);
    assert!(tab);
    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Input);
    let backtab =
        dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::BackTab), None);
    assert!(backtab);
    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Transcript);
}

#[test]
fn normal_mode_typing_block_table() {
    // Busy normal mode blocks plain typing until an explicit i.
    let mut state = busy_state();
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);
    let esc = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(esc);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::AbortPending);
    let typed_while_normal = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );
    assert!(!typed_while_normal);
    assert_eq!(state.input.mode, InputMode::Normal);
    let enter_insert = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('i')),
        None,
    );
    assert!(enter_insert);
    assert_eq!(state.input.mode, InputMode::Insert);
    let typed_after_i = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );
    assert!(!typed_after_i);

    // Busy normal mode after the jk chord requires i before typing.
    let mut state = busy_state();
    let first_j = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(first_j);
    let second_k = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(second_k);
    assert_eq!(state.input.mode, InputMode::Normal);
    let typed_while_normal = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('z')),
        None,
    );
    assert!(!typed_while_normal);
    let enter_insert = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('i')),
        None,
    );
    assert!(enter_insert);
    assert_eq!(state.input.mode, InputMode::Insert);
    let typed_after_i = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('z')),
        None,
    );
    assert!(!typed_after_i);

    // Normal mode blocks plain typing and keeps the input unchanged.
    let mut state = AppState::default();
    state.enter_normal_mode();
    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );
    assert!(!changed);
    assert_eq!(state.input.mode, InputMode::Normal);

    // Normal mode z is a no-op.
    let mut state = AppState::default();
    state.enter_normal_mode();
    let z = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('z')),
        None,
    );
    assert!(!z);
}
