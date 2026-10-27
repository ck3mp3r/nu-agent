//! The cancel branch publishes `PermissionRequestCancelled`.
//!
//! A cancel that arrives while a permission request is pending resolves the
//! request as a denial and publishes its own event. No decision exists at that
//! moment, so the event carries the request id and the rule identity only.

use std::time::Duration;

use super::*;

/// A confirmed cancel must publish the cancelled event with the pending
/// request's id and the rule identity shown on the card, and must still return
/// the turn-cancelled denial.
#[tokio::test]
async fn test_hook_permission_resolver_permission_request_cancelled_publishes_on_cancel()
-> Result<()> {
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
    let (request_id, prompted_rule) = match event {
        UiEvent::PermissionRequested {
            request_id,
            context,
        } => (request_id, context.matched_rule_identity),
        other => panic!("expected PermissionRequested, got {other:?}"),
    };

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
    let event = tokio::time::timeout(Duration::from_secs(2), ui_rx.recv())
        .await
        .map_err(|_| "timed out waiting for PermissionRequestCancelled")?
        .map_err(|_| "expected a PermissionRequestCancelled event")?;
    match event {
        UiEvent::PermissionRequestCancelled {
            request_id: cancelled_id,
            matched_rule_identity,
        } => {
            assert_eq!(
                cancelled_id, request_id,
                "the cancelled event must name the pending request"
            );
            assert_eq!(
                matched_rule_identity, prompted_rule,
                "the cancelled event must carry the rule identity shown on the card"
            );
        }
        other => panic!("expected PermissionRequestCancelled, got {other:?}"),
    }
    Ok(())
}

/// A request that is answered publishes no cancelled event.
#[tokio::test]
async fn test_hook_permission_resolver_permission_request_cancelled_absent_without_cancel()
-> Result<()> {
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
        UiEvent::PermissionRequested { request_id, .. } => request_id,
        other => panic!("expected PermissionRequested, got {other:?}"),
    };

    // -- Exec
    submit_decision(
        &resolver,
        &request_id,
        ProtocolPermissionDecision::AllowOnce,
    );
    let decision = tokio::time::timeout(Duration::from_secs(2), resolve_task)
        .await
        .map_err(|_| "resolve never returned after the decision")?
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    assert_eq!(decision, PermissionDecision::Allow);
    let events = drain(&mut ui_rx);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, UiEvent::PermissionRequestCancelled { .. })),
        "an answered request publishes no cancelled event: {events:?}"
    );
    Ok(())
}
