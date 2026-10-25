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
    let mut ui_rx = bus.ui_event().subscribe();
    let resolver_for_task = resolver.clone();
    let bus_for_task = bus.clone();
    let resolve_task = tokio::spawn(async move {
        resolver_for_task
            .resolve(ASK_TOOL, "{}", None, &bus_for_task)
            .await
    });

    let event = tokio::time::timeout(Duration::from_secs(2), ui_rx.recv())
        .await
        .map_err(|_| "timed out waiting for PermissionRequested")?
        .map_err(|_| "expected a PermissionRequested event")?;
    let request_id = match event {
        UiEvent::PermissionRequested { request_id, .. } => request_id,
        other => panic!("expected PermissionRequested, got {other:?}"),
    };
    assert!(
        resolver
            .pending
            .lock()
            .map_err(|_| "pending lock poisoned")?
            .contains_key(&request_id),
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

    let event = tokio::time::timeout(Duration::from_secs(2), ui_rx.recv())
        .await
        .map_err(|_| "timed out waiting for PermissionDecisionTimedOut")?
        .map_err(|_| "expected a PermissionDecisionTimedOut event")?;
    match event {
        UiEvent::PermissionDecisionTimedOut {
            request_id: timed_out_id,
        } => assert_eq!(
            timed_out_id, request_id,
            "the timeout event must name the timed-out request"
        ),
        other => panic!("expected PermissionDecisionTimedOut, got {other:?}"),
    }
    Ok(())
}

/// A disabled timeout (`None`) must leave the request waiting indefinitely.
#[tokio::test]
async fn test_hook_permission_resolver_timeout_disabled_waits() -> Result<()> {
    // -- Setup & Fixtures
    let permissions = ask_global_with_read_allowed_config().with_decision_timeout(None);
    let (resolver, bus) = make_interactive(permissions);
    let mut ui_rx = bus.ui_event().subscribe();
    let resolver_for_task = resolver.clone();
    let bus_for_task = bus.clone();
    let mut resolve_task = tokio::spawn(async move {
        resolver_for_task
            .resolve(ASK_TOOL, "{}", None, &bus_for_task)
            .await
    });

    let event = tokio::time::timeout(Duration::from_secs(2), ui_rx.recv())
        .await
        .map_err(|_| "timed out waiting for PermissionRequested")?
        .map_err(|_| "expected a PermissionRequested event")?;
    let request_id = match event {
        UiEvent::PermissionRequested { request_id, .. } => request_id,
        other => panic!("expected PermissionRequested, got {other:?}"),
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
            .contains_key(&request_id),
        "the request must still be pending while the timeout is disabled"
    );
    resolve_task.abort();
    Ok(())
}
