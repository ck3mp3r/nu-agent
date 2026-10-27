//! The `permission_decision_submitted` event: payload shape and subscription.

use super::*;

#[tokio::test]
async fn test_hooks_runner_permission_decision_submitted_invokes_with_payload() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionDecisionSubmitted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .ui_event()
        .send(UiEvent::PermissionDecisionSubmitted {
            request_id: "req-1".to_string(),
            decision: PermissionDecision::AllowOnce,
            matched_rule_identity: "permissions.nu".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "a submitted-only hook must subscribe ui_event; got: {calls:?}"
    );
    assert_eq!(calls[0].event, "permission_decision_submitted");
    let body = payload(&calls[0])?;
    assert_eq!(body["request_id"], "req-1");
    assert_eq!(body["decision"], "allow_once");
    assert_eq!(body["matched_rule_identity"], "permissions.nu");
    assert_eq!(
        body.as_object().map(|object| object.len()),
        Some(3),
        "the payload carries the variant fields verbatim: {body}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_permission_decision_submitted_encodes_every_decision() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionDecisionSubmitted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    for decision in [
        PermissionDecision::AllowOnce,
        PermissionDecision::AllowAlways,
        PermissionDecision::Deny,
    ] {
        let _ = bus
            .ui_event()
            .send(UiEvent::PermissionDecisionSubmitted {
                request_id: "req-1".to_string(),
                decision,
                matched_rule_identity: "permissions.nu".to_string(),
            })
            .await;
    }
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(calls.len(), 3, "every decision must invoke the hook");
    for (call, expected) in calls.iter().zip(["allow_once", "allow_always", "deny"]) {
        assert_eq!(payload(call)?["decision"], expected);
    }
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_permission_decision_submitted_stamps_session_id() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionDecisionSubmitted])];
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
        .send(UiEvent::PermissionDecisionSubmitted {
            request_id: "req-1".to_string(),
            decision: PermissionDecision::Deny,
            matched_rule_identity: "permissions.nu".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "only the submitted event invokes; got: {calls:?}"
    );
    assert_eq!(
        envelope_json(&calls[0])?["session_id"],
        "sess-1",
        "Started must stamp the tracked session id"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_permission_decision_submitted_ignores_other_decisions() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionDecisionSubmitted])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .ui_event()
        .send(UiEvent::PermissionDecisionTimedOut {
            request_id: "req-1".to_string(),
        })
        .await;
    let _ = bus
        .ui_event()
        .send(UiEvent::PermissionDecisionIgnored {
            request_id: "req-1".to_string(),
            reason: "turn cancelled".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    assert!(
        process.calls().is_empty(),
        "a timeout or an ignored decision must not fire permission_decision_submitted"
    );
    Ok(())
}
