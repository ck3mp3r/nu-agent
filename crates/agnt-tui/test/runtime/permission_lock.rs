//! Permission-card keys under the input lock, exercised through the real
//! insert-mode key path so the textarea is observable.

use super::*;

/// A card key approves the card while the turn holds the input lock, and a
/// non-card Char key leaves the textarea untouched.
#[test]
fn coordinator_permission_card_keys_bypass_input_lock() -> Result<()> {
    // -- Setup & Fixtures: locked, card up, seeded textarea.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.state.input_locked = true;
    coordinator.textarea.insert_str("seeded");
    coordinator.state.permission.open_prompt(PermissionPrompt {
        request_id: "ask-0000000000000001".to_string(),
        matched_rule_identity: "nested:nu.command:*".to_string(),
        tool: "nu".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        scope: "nested".to_string(),
        pattern: "*".to_string(),
        target_field: Some("command".to_string()),
        summary: "→ {\"command\":\"echo hi\"}".to_string(),
    });

    // -- Exec: a non-card Char key.
    coordinator.handle_terminal_event(TerminalEvent::Key(TerminalKey::Char('x')));

    // -- Check: no decision, card open, textarea unchanged.
    assert!(coordinator.state.permission.has_prompt());
    assert!(
        coordinator
            .state
            .permission
            .take_next_submission()
            .is_none(),
        "a non-card key must not submit a permission decision"
    );
    assert_eq!(coordinator.textarea.lines().join("\n"), "seeded");

    // -- Exec: the approval key.
    coordinator.handle_terminal_event(TerminalEvent::Key(TerminalKey::Char('a')));

    // -- Check: the card is approved and the textarea is still untouched.
    assert!(!coordinator.state.permission.has_prompt());
    let submission = coordinator
        .state
        .permission
        .take_next_submission()
        .ok_or("should have permission submission")?;
    assert_eq!(submission.decision, PermissionDecision::AllowOnce);
    assert_eq!(coordinator.textarea.lines().join("\n"), "seeded");
    Ok(())
}

/// A paste while the turn holds the input lock and no card is up is dropped:
/// the textarea keeps its seeded content, matching the typed-key path where a
/// Char key is consumed without a textarea write.
#[test]
fn coordinator_paste_dropped_while_input_locked_without_card() -> Result<()> {
    // -- Setup & Fixtures: locked, no card, seeded textarea.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.state.input_locked = true;
    coordinator.textarea.insert_str("seeded");
    assert!(!coordinator.state.permission.has_prompt());

    // -- Exec
    coordinator.handle_terminal_event(TerminalEvent::Paste("pasted".to_string()));

    // -- Check: the textarea is untouched and no card appeared.
    assert_eq!(coordinator.textarea.lines().join("\n"), "seeded");
    assert!(!coordinator.state.permission.has_prompt());
    Ok(())
}

/// A paste while a permission card is up is dropped: the textarea keeps its
/// seeded content and the card submits no decision, matching the typed-key
/// path where the card owns every key.
#[test]
fn coordinator_paste_dropped_while_permission_card_up() -> Result<()> {
    // -- Setup & Fixtures: unlocked, card up, seeded textarea.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator.textarea.insert_str("seeded");
    coordinator.state.permission.open_prompt(PermissionPrompt {
        request_id: "ask-0000000000000002".to_string(),
        matched_rule_identity: "nested:nu.command:*".to_string(),
        tool: "nu".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        scope: "nested".to_string(),
        pattern: "*".to_string(),
        target_field: Some("command".to_string()),
        summary: "→ {\"command\":\"echo hi\"}".to_string(),
    });

    // -- Exec
    coordinator.handle_terminal_event(TerminalEvent::Paste("pasted".to_string()));

    // -- Check: the textarea is untouched, the card stays up, no decision.
    assert_eq!(coordinator.textarea.lines().join("\n"), "seeded");
    assert!(coordinator.state.permission.has_prompt());
    assert!(
        coordinator
            .state
            .permission
            .take_next_submission()
            .is_none(),
        "a paste must not submit a permission decision"
    );
    Ok(())
}
