use super::*;

#[test]
fn esc_in_idle_insert_mode_switches_to_normal_mode() {
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);

    assert!(changed);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Idle);
}

#[test]
fn jj_chord_in_busy_insert_mode_switches_to_normal_mode() {
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

    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(second);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Busy);
}

#[test]
fn jk_chord_in_busy_insert_mode_switches_to_normal_mode() {
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

    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(second);
    assert_eq!(state.input.mode, InputMode::Normal);
    assert_eq!(state.phase, UiPhase::Busy);
}

#[test]
fn busy_normal_mode_blocks_plain_typing_until_explicit_i() {
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
    assert!(!typed_after_i); // InsertChar is now a no-op in dispatch
}

#[test]
fn busy_normal_mode_after_jk_chord_requires_i_before_typing() {
    let mut state = busy_state();
    assert_eq!(state.phase, UiPhase::Busy);
    assert_eq!(state.input.mode, InputMode::Insert);

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
    assert!(!typed_after_i); // InsertChar is now a no-op in dispatch
}

#[test]
fn normal_mode_blocks_plain_typing_and_keeps_input_unchanged() {
    let mut state = AppState::default();
    state.enter_normal_mode();

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );

    assert!(!changed);
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn normal_mode_hl_cycles_focus_between_panes() {
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
}

#[test]
fn normal_mode_tab_and_backtab_cycle_focus_between_transcript_and_input() {
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
fn insert_mode_jk_chord_enters_normal_and_removes_j() {
    let mut state = AppState::default();
    assert_eq!(state.input.mode, InputMode::Insert);

    // InsertChar is now a no-op in the dispatch path (handled by TextArea).
    // The first 'j' sets insert_exit_pending_j but returns false (no-op).
    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(!first);
    assert_eq!(state.input.mode, InputMode::Insert);

    // The second 'k' triggers EnterNormalModeFromChord which still works.
    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(second);
    assert_eq!(state.input.mode, InputMode::Normal);
}

#[test]
fn normal_mode_z_is_noop() {
    let mut state = AppState::default();
    state.enter_normal_mode();

    let z = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('z')),
        None,
    );
    assert!(!z);
}

#[test]
fn existing_insert_mode_jk_chord_still_switches_to_normal_outside_palette() {
    let mut state = AppState::default();
    // InsertChar is now a no-op in the dispatch path (handled by TextArea).
    // The first 'j' sets insert_exit_pending_j but returns false (no-op).
    let first = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('j')),
        None,
    );
    assert!(!first);
    // The second 'k' triggers EnterNormalModeFromChord which still works.
    let second = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('k')),
        None,
    );
    assert!(second);
    assert_eq!(state.input.mode, InputMode::Normal);
}
