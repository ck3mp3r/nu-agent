use super::*;

#[test]
fn permission_prompt_decision_table() -> Result<()> {
    let cases = [
        (TerminalKey::Char('a'), PermissionDecision::AllowOnce),
        (TerminalKey::Char('A'), PermissionDecision::AllowAlways),
        (TerminalKey::Char('d'), PermissionDecision::Deny),
        (TerminalKey::Esc, PermissionDecision::Deny),
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
