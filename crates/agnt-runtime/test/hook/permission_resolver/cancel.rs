use std::time::Duration;

use super::*;

use crate::bus::RequestCancel;

/// A cancel confirmed while a permission request is pending must resolve the
/// request as a denial instead of waiting for a decision that never comes.
#[tokio::test]
async fn test_hook_permission_resolver_cancel_while_awaiting_denies() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
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
