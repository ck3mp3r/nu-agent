use super::*;

#[test]
fn permission_prompt_decision_table() -> Result<()> {
    let cases = [
        (TerminalKey::Char('a'), PermissionDecision::AllowOnce),
        (TerminalKey::Char('A'), PermissionDecision::AllowAlways),
        (TerminalKey::Char('d'), PermissionDecision::Deny),
    ];

    for (key, expected) in cases {
        let mut state = AppState::default();
        open_permission_prompt(&mut state);

        let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(key), None);
        assert!(changed);
        assert!(!state.permission.has_prompt());

        let submission = state
            .permission
            .take_next_submission()
            .ok_or("should have permission submission")?;
        assert_eq!(submission.decision, expected);
    }
    Ok(())
}

/// Esc is not a card key: with a card up in normal mode it submits no decision
/// and the card stays open.
#[test]
fn test_interaction_permission_prompt_esc_submits_no_decision() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        ..Default::default()
    };
    open_permission_prompt(&mut state);

    // -- Exec
    dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);

    // -- Check
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

/// A permission card stays approvable while a turn holds the input lock: the
/// card keys must reach the card arm of `rewrite_action` even though every
/// other Char key is dropped.
#[test]
fn test_interaction_permission_prompt_locked_decision_table() -> Result<()> {
    // -- Setup & Fixtures
    let cases = [
        (TerminalKey::Char('a'), PermissionDecision::AllowOnce),
        (TerminalKey::Char('A'), PermissionDecision::AllowAlways),
        (TerminalKey::Char('d'), PermissionDecision::Deny),
    ];

    for (key, expected) in cases {
        let mut state = AppState {
            input_locked: true,
            ..Default::default()
        };
        open_permission_prompt(&mut state);

        // -- Exec
        let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(key), None);

        // -- Check
        assert!(changed, "locked card key {key:?} should report a change");
        assert!(
            !state.permission.has_prompt(),
            "locked card key {key:?} should close the card"
        );
        let submission = state
            .permission
            .take_next_submission()
            .ok_or("should have permission submission")?;
        assert_eq!(submission.decision, expected);
    }
    Ok(())
}

/// While the card is up, a non-card Char key submits no decision and leaves the
/// card open — the suspension of the lock must not widen the card's key set.
#[test]
fn test_interaction_permission_prompt_locked_other_char_noop() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState {
        input_locked: true,
        ..Default::default()
    };
    open_permission_prompt(&mut state);

    // -- Exec
    dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('x')),
        None,
    );

    // -- Check: the card arm rewrites the key to Noop, so the card stays up and
    // no decision is submitted.
    assert!(state.permission.has_prompt());
    assert!(
        state.permission.take_next_submission().is_none(),
        "a non-card key must not submit a permission decision"
    );
    Ok(())
}

/// With no card up the lock still drops Char keys: a normal-mode `i` must not
/// enter insert mode while the turn holds the lock.
#[test]
fn test_interaction_permission_prompt_locked_no_card_drops_char() -> Result<()> {
    // -- Setup & Fixtures: locked, no card.
    let mut locked = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        input_locked: true,
        ..Default::default()
    };

    // -- Exec
    dispatch_terminal_event(
        &mut locked,
        &TerminalEvent::Key(TerminalKey::Char('i')),
        None,
    );

    // -- Check: the key was dropped.
    assert_eq!(locked.input.mode, InputMode::Normal);

    // -- Setup & Fixtures: the same key with the lock released.
    let mut unlocked = AppState {
        input: InputState::default().with_mode(InputMode::Normal),
        ..Default::default()
    };

    // -- Exec
    dispatch_terminal_event(
        &mut unlocked,
        &TerminalEvent::Key(TerminalKey::Char('i')),
        None,
    );

    // -- Check: the control case proves the key is otherwise live.
    assert_eq!(unlocked.input.mode, InputMode::Insert);
    Ok(())
}
