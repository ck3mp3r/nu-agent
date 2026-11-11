//! The `permission_decision_ignored` event: payload shape and subscription.

use super::*;

#[tokio::test]
async fn test_hooks_runner_permission_decision_ignored_invokes_with_payload() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionDecisionIgnored])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .stream()
        .publish(SessionRecord::Permission(PermissionRecord::Ignored {
            request_id: KernelRequestId::from_minted("req-1"),
            reason: "request_not_pending".to_string(),
        }));
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "an ignored-only hook must subscribe the stream; got: {calls:?}"
    );
    assert_eq!(calls[0].event, "permission_decision_ignored");
    let body = payload(&calls[0])?;
    assert_eq!(body["request_id"], "req-1");
    assert_eq!(body["reason"], "request_not_pending");
    assert_eq!(
        body.as_object().map(|object| object.len()),
        Some(2),
        "the payload carries the variant fields verbatim: {body}"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_permission_decision_ignored_stamps_session_id() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionDecisionIgnored])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus.lifecycle().publish(LifecycleEvent::SessionStarted {
        session_id: "sess-1".to_string(),
        hydrated: false,
    });
    let _ = bus
        .stream()
        .publish(SessionRecord::Permission(PermissionRecord::Ignored {
            request_id: KernelRequestId::from_minted("req-1"),
            reason: "request_not_pending".to_string(),
        }));
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "only the ignored event invokes; got: {calls:?}"
    );
    assert_eq!(
        envelope_json(&calls[0])?["session_id"],
        "sess-1",
        "Started must stamp the tracked session id"
    );
    Ok(())
}

#[tokio::test]
async fn test_hooks_runner_permission_decision_ignored_ignores_other_decisions() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionDecisionIgnored])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .stream()
        .publish(SessionRecord::Permission(PermissionRecord::TimedOut {
            request_id: KernelRequestId::from_minted("req-1"),
        }));
    let _ = bus
        .stream()
        .publish(SessionRecord::Permission(PermissionRecord::Decided {
            request_id: KernelRequestId::from_minted("req-1"),
            decision: PermissionDecision::AllowOnce,
            rule_identity: "permissions.nu".to_string(),
        }));
    drop(bus);
    finish(handles).await?;

    // -- Check
    assert!(
        process.calls().is_empty(),
        "a timeout or a submitted decision must not fire permission_decision_ignored"
    );
    Ok(())
}
