use super::*;

#[test]
fn permission_prompt_key_a_submits_allow_once() -> Result<()> {
    let mut state = AppState::default();
    open_permission_prompt(&mut state);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('a')),
        None,
    );
    assert!(changed);
    assert!(!state.permission.has_prompt());

    let submission = state
        .permission
        .take_next_submission()
        .ok_or("should have permission submission")?;
    assert_eq!(submission.decision, PermissionDecision::AllowOnce);
    Ok(())
}

#[test]
fn permission_prompt_key_upper_a_submits_allow_always() -> Result<()> {
    let mut state = AppState::default();
    open_permission_prompt(&mut state);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('A')),
        None,
    );
    assert!(changed);
    assert!(!state.permission.has_prompt());

    let submission = state
        .permission
        .take_next_submission()
        .ok_or("should have permission submission")?;
    assert_eq!(submission.decision, PermissionDecision::AllowAlways);
    Ok(())
}

#[test]
fn permission_prompt_key_d_submits_deny() -> Result<()> {
    let mut state = AppState::default();
    open_permission_prompt(&mut state);

    let changed = dispatch_terminal_event(
        &mut state,
        &TerminalEvent::Key(TerminalKey::Char('d')),
        None,
    );
    assert!(changed);
    assert!(!state.permission.has_prompt());

    let submission = state
        .permission
        .take_next_submission()
        .ok_or("should have permission submission")?;
    assert_eq!(submission.decision, PermissionDecision::Deny);
    Ok(())
}

#[test]
fn permission_prompt_esc_submits_deny() -> Result<()> {
    let mut state = AppState::default();
    open_permission_prompt(&mut state);

    let changed = dispatch_terminal_event(&mut state, &TerminalEvent::Key(TerminalKey::Esc), None);
    assert!(changed);
    assert!(!state.permission.has_prompt());

    let submission = state
        .permission
        .take_next_submission()
        .ok_or("should have permission submission")?;
    assert_eq!(submission.decision, PermissionDecision::Deny);
    Ok(())
}
