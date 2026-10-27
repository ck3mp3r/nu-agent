//! The `permission_auto_resolved` event: payload shape and subscription.

use super::*;

#[tokio::test]
async fn test_hooks_runner_permission_auto_resolved_invokes_with_payload() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionAutoResolved])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .ui_event()
        .send(UiEvent::PermissionAutoResolved {
            matched_rule_identity: "tool:read".to_string(),
            action: "allow".to_string(),
            reason: None,
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "an auto-resolved-only hook must subscribe ui_event; got: {calls:?}"
    );
    assert_eq!(calls[0].event, "permission_auto_resolved");
    let body = payload(&calls[0])?;
    assert_eq!(body["matched_rule_identity"], "tool:read");
    assert_eq!(body["action"], "allow");
    assert_eq!(
        body["reason"],
        serde_json::Value::Null,
        "an allow carries a null reason: {body}"
    );
    assert_eq!(
        body.as_object().map(|object| object.len()),
        Some(3),
        "the payload carries the variant fields verbatim: {body}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_permission_auto_resolved_carries_deny_reason() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionAutoResolved])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .ui_event()
        .send(UiEvent::PermissionAutoResolved {
            matched_rule_identity: "global:*".to_string(),
            action: "deny".to_string(),
            reason: Some("Permission denied by rule 'global:*' (scope: global)".to_string()),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 1, "the deny event must invoke; got: {calls:?}");
    let body = payload(&calls[0])?;
    assert_eq!(body["action"], "deny");
    assert_eq!(
        body["reason"],
        "Permission denied by rule 'global:*' (scope: global)"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_permission_auto_resolved_stamps_session_id() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionAutoResolved])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .session()
        .send(SessionEvent::Started {
            session_id: "sess-1".to_string(),
            hydrated: false,
        })
        .await;
    let _ = bus
        .ui_event()
        .send(UiEvent::PermissionAutoResolved {
            matched_rule_identity: "tool:read".to_string(),
            action: "allow".to_string(),
            reason: None,
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "only the auto-resolved event invokes; got: {calls:?}"
    );
    assert_eq!(
        envelope_json(&calls[0])?["session_id"],
        "sess-1",
        "Started must stamp the tracked session id"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_permission_auto_resolved_ignores_prompt_events() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionAutoResolved])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .ui_event()
        .send(UiEvent::PermissionRequested {
            request_id: "req-1".to_string(),
            context: PermissionRequestContext {
                tool: "nu".to_string(),
                tool_key: "nu\n{}".to_string(),
                source: "builtin".to_string(),
                mode: None,
                matched_rule_identity: "global:*".to_string(),
                scope: "global".to_string(),
                target_field: None,
                pattern: "*".to_string(),
                summary: "→ {}".to_string(),
                pre_authorize_display: None,
            },
        })
        .await;
    let _ = bus
        .ui_event()
        .send(UiEvent::PermissionDecisionSubmitted {
            request_id: "req-1".to_string(),
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: "global:*".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    assert!(
        process.calls().is_empty(),
        "a prompt or a submitted decision must not fire permission_auto_resolved"
    );
    Ok(())
}
