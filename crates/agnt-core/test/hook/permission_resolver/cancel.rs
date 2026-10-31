use std::time::Duration;

use super::*;

/// A cancel confirmed while a permission request is pending must resolve the
/// request as a denial instead of waiting for a decision that never comes.
#[tokio::test]
async fn test_hook_permission_resolver_cancel_while_awaiting_denies() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
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
        SystemEvent::PermissionRequested { request_id, .. } => request_id,
        other => panic!("expected PermissionRequested, got {other:?}"),
    };
    assert!(
        resolver
            .pending
            .lock()
            .map_err(|_| "pending lock poisoned")?
            .contains_key(request_id.as_str()),
        "the request must be pending before the cancel"
    );

    // -- Exec
    bus.cancel()
        .request_cancel()
        .await
        .map_err(|e| format!("resolve must subscribe to the cancel channel: {e}"))?;
    let decision = tokio::time::timeout(Duration::from_secs(2), resolve_task)
        .await
        .map_err(|_| "resolve never returned after the cancel")?
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    match decision {
        PermissionDecision::Deny { reason } => assert_eq!(
            reason, "Permission denied: turn cancelled by user",
            "the cancel denial must carry the cancelled-turn reason"
        ),
        other => panic!("expected Deny, got {other:?}"),
    }
    assert!(
        resolver
            .pending
            .lock()
            .map_err(|_| "pending lock poisoned")?
            .is_empty(),
        "the cancelled request must be removed from the pending map"
    );
    Ok(())
}
