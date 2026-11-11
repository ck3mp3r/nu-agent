//! The cancel branch publishes `PermissionRecord::Cancelled`.
//!
//! A cancel that arrives while a permission request is pending resolves the
//! request as a denial and records its own row. No decision exists at that
//! moment, so the record carries the request id and the rule identity only.

use std::time::Duration;

use super::*;

use crate::bus::RequestCancel;

/// A confirmed cancel must record the cancelled row with the pending request's
/// id and the rule identity shown on the card, and must still return the
/// turn-cancelled denial.
#[tokio::test]
async fn test_hook_permission_resolver_permission_request_cancelled_publishes_on_cancel()
-> Result<()> {
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
    let SessionRecord::Permission(PermissionRecord::Requested {
        request_id,
        rule_identity: prompted_rule,
        ..
    }) = record
    else {
        panic!("expected a Requested permission record");
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
    let record = tokio::time::timeout(Duration::from_secs(2), stream_rx.recv())
        .await
        .map_err(|_| "timed out waiting for the cancelled row")?
        .map_err(|_| "expected a cancelled row")?;
    let SessionRecord::Permission(PermissionRecord::Cancelled {
        request_id: cancelled_id,
        rule_identity,
    }) = record
    else {
        panic!("expected a Cancelled permission record");
    };
    assert_eq!(
        cancelled_id, request_id,
        "the cancelled row must name the pending request"
    );
    assert_eq!(
        rule_identity, prompted_rule,
        "the cancelled row must carry the rule identity shown on the card"
    );
    Ok(())
}

/// A request that is answered records no cancelled row.
#[tokio::test]
async fn test_hook_permission_resolver_permission_request_cancelled_absent_without_cancel()
-> Result<()> {
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
    let request_id = request_id_of(&permissions(vec![record]))?;

    // -- Exec
    submit_decision(
        &resolver,
        request_id.as_str(),
        ProtocolPermissionDecision::AllowOnce,
    );
    let decision = tokio::time::timeout(Duration::from_secs(2), resolve_task)
        .await
        .map_err(|_| "resolve never returned after the decision")?
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    assert_eq!(decision, PermissionDecision::Allow);
    let records = permissions(drain(&mut stream_rx));
    assert!(
        !records
            .iter()
            .any(|record| matches!(record, PermissionRecord::Cancelled { .. })),
        "an answered request records no cancelled row: {records:?}"
    );
    Ok(())
}
