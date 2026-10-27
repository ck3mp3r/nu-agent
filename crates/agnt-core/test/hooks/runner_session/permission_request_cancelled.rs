//! The `permission_request_cancelled` event: payload shape and subscription.

use super::*;

#[tokio::test]
async fn test_hooks_runner_permission_request_cancelled_invokes_with_payload() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionRequestCancelled])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .ui_event()
        .send(UiEvent::PermissionRequestCancelled {
            request_id: "req-1".to_string(),
            matched_rule_identity: "global:*".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "a cancelled-only hook must subscribe ui_event; got: {calls:?}"
    );
    assert_eq!(calls[0].event, "permission_request_cancelled");
    let body = payload(&calls[0])?;
    assert_eq!(body["request_id"], "req-1");
    assert_eq!(body["matched_rule_identity"], "global:*");
    assert_eq!(
        body.as_object().map(|object| object.len()),
        Some(2),
        "the payload carries the variant fields verbatim: {body}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_permission_request_cancelled_stamps_session_id() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionRequestCancelled])];
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
        .send(UiEvent::PermissionRequestCancelled {
            request_id: "req-1".to_string(),
            matched_rule_identity: "global:*".to_string(),
        })
        .await;
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "only the cancelled event invokes; got: {calls:?}"
    );
    assert_eq!(
        envelope_json(&calls[0])?["session_id"],
        "sess-1",
        "Started must stamp the tracked session id"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_permission_request_cancelled_ignores_other_permission_events()
-> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionRequestCancelled])];
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
        "a timeout or a submitted decision must not fire permission_request_cancelled"
    );
    Ok(())
}
