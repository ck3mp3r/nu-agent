//! The `permission_request_cancelled` event: payload shape and subscription.

use super::*;

#[tokio::test]
async fn test_hooks_runner_permission_request_cancelled_invokes_with_payload() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionRequestCancelled])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus
        .stream()
        .publish(SessionRecord::Permission(PermissionRecord::Cancelled {
            request_id: KernelRequestId::from_minted("req-1"),
            rule_identity: "global:*".to_string(),
        }));
    drop(bus);
    finish(handles).await?;

    // -- Check
    let calls = process.calls();
    assert_eq!(
        calls.len(),
        1,
        "a cancelled-only hook must subscribe the stream; got: {calls:?}"
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
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionRequestCancelled])];
    let handles = spawn(&bus, &hooks, &process);

    // -- Exec
    let _ = bus.lifecycle().publish(LifecycleEvent::SessionStarted {
        session_id: "sess-1".to_string(),
        hydrated: false,
    });
    let _ = bus
        .stream()
        .publish(SessionRecord::Permission(PermissionRecord::Cancelled {
            request_id: KernelRequestId::from_minted("req-1"),
            rule_identity: "global:*".to_string(),
        }));
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
    let bus = Bus::default();
    let process = MockProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::PermissionRequestCancelled])];
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
            rule_identity: "global:*".to_string(),
        }));
    drop(bus);
    finish(handles).await?;

    // -- Check
    assert!(
        process.calls().is_empty(),
        "a timeout or a submitted decision must not fire permission_request_cancelled"
    );
    Ok(())
}
