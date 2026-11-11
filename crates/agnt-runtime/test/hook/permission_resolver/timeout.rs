use std::time::Duration;

use super::*;

/// An unanswered permission request must resolve as a denial once the
/// configured decision timeout elapses, and must announce the timeout.
#[tokio::test]
async fn test_hook_permission_resolver_timeout_while_awaiting_denies() -> Result<()> {
    // -- Setup & Fixtures
    let permissions = ask_global_with_read_allowed_config()
        .with_decision_timeout(Some(Duration::from_millis(50)));
    let (resolver, bus) = make_interactive(permissions);
    let mut stream_rx = bus.stream().subscribe();
    let resolver_for_task = resolver.clone();
    let bus_for_task = bus.clone();
    let resolve_task = tokio::spawn(async move {
        resolver_for_task
            .resolve(
                ASK_TOOL,
                "{}",
                None,
                bus_for_task.stream(),
                bus_for_task.facts(),
                bus_for_task.cancel(),
            )
            .await
    });

    let record = tokio::time::timeout(Duration::from_secs(2), stream_rx.recv())
        .await
        .map_err(|_| "timed out waiting for the gate record")?
        .map_err(|_| "expected a gate record")?;
    let SessionRecord::Permission(PermissionRecord::Requested { request_id, .. }) = record else {
        panic!("expected a Requested permission record");
    };
    assert!(
        resolver
            .pending
            .lock()
            .map_err(|_| "pending lock poisoned")?
            .contains_key(request_id.as_str()),
        "the request must be pending before the timeout"
    );

    // -- Exec
    let decision = tokio::time::timeout(Duration::from_secs(2), resolve_task)
        .await
        .map_err(|_| "resolve never returned after the decision timeout")?
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    match decision {
        PermissionDecision::Deny { reason } => assert!(
            reason.starts_with("Permission denied"),
            "the timeout denial must keep the documented prefix, got '{reason}'"
        ),
        other => panic!("expected Deny, got {other:?}"),
    }
    assert!(
        resolver
            .pending
            .lock()
            .map_err(|_| "pending lock poisoned")?
            .is_empty(),
        "the timed-out request must be removed from the pending map"
    );

    let record = tokio::time::timeout(Duration::from_secs(2), stream_rx.recv())
        .await
        .map_err(|_| "timed out waiting for the timed-out record")?
        .map_err(|_| "expected a timed-out record")?;
    let SessionRecord::Permission(PermissionRecord::TimedOut {
        request_id: timed_out_id,
    }) = record
    else {
        panic!("expected a TimedOut permission record");
    };
    assert_eq!(
        timed_out_id, request_id,
        "the timeout record must name the timed-out request"
    );
    Ok(())
}

/// A disabled timeout (`None`) must leave the request waiting indefinitely.
#[tokio::test]
async fn test_hook_permission_resolver_timeout_disabled_waits() -> Result<()> {
    // -- Setup & Fixtures
    let permissions = ask_global_with_read_allowed_config().with_decision_timeout(None);
    let (resolver, bus) = make_interactive(permissions);
    let mut stream_rx = bus.stream().subscribe();
    let resolver_for_task = resolver.clone();
    let bus_for_task = bus.clone();
    let mut resolve_task = tokio::spawn(async move {
        resolver_for_task
            .resolve(
                ASK_TOOL,
                "{}",
                None,
                bus_for_task.stream(),
                bus_for_task.facts(),
                bus_for_task.cancel(),
            )
            .await
    });

    let record = tokio::time::timeout(Duration::from_secs(2), stream_rx.recv())
        .await
        .map_err(|_| "timed out waiting for the gate record")?
        .map_err(|_| "expected a gate record")?;
    let SessionRecord::Permission(PermissionRecord::Requested { request_id, .. }) = record else {
        panic!("expected a Requested permission record");
    };

    // -- Exec
    let early = tokio::time::timeout(Duration::from_millis(200), &mut resolve_task).await;

    // -- Check
    assert!(
        early.is_err(),
        "a disabled timeout must not resolve the request"
    );
    assert!(
        resolver
            .pending
            .lock()
            .map_err(|_| "pending lock poisoned")?
            .contains_key(request_id.as_str()),
        "the request must still be pending while the timeout is disabled"
    );
    resolve_task.abort();
    Ok(())
}
