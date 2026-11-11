//! The decision funnel records `PermissionRecord::Decided`.
//!
//! The funnel is the single point every prompted decision passes through, so
//! one row there covers the TUI and ACP modes. The timeout and cancel branches
//! return before the funnel, so they must record nothing of their own.

use std::time::Duration;

use super::*;

use crate::bus::RequestCancel;

/// A submitted decision must reach the stream with the request id, the decision,
/// and the identity of the rule that produced the prompt.
#[tokio::test]
async fn test_hook_permission_resolver_submitted_publishes_decision_and_rule() -> Result<()> {
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
    let record = tokio::time::timeout(Duration::from_secs(2), stream_rx.recv())
        .await
        .map_err(|_| "timed out waiting for the decided row")?
        .map_err(|_| "expected a decided row")?;
    let SessionRecord::Permission(PermissionRecord::Decided {
        request_id: submitted_id,
        decision: submitted_decision,
        rule_identity,
    }) = record
    else {
        panic!("expected a Decided permission record");
    };
    assert_eq!(
        submitted_id, request_id,
        "the decided row must name the answered request"
    );
    assert_eq!(
        submitted_decision,
        ProtocolPermissionDecision::AllowOnce,
        "the decided row must carry the user's decision"
    );
    assert_eq!(
        rule_identity, prompted_rule,
        "the decided row must carry the rule identity shown on the card"
    );
    Ok(())
}

/// A denied decision must be recorded as a deny.
#[tokio::test]
async fn test_hook_permission_resolver_submitted_publishes_deny() -> Result<()> {
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
        ProtocolPermissionDecision::Deny,
    );
    let decision = tokio::time::timeout(Duration::from_secs(2), resolve_task)
        .await
        .map_err(|_| "resolve never returned after the decision")?
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    assert!(
        matches!(decision, PermissionDecision::Deny { .. }),
        "expected Deny, got {decision:?}"
    );
    let record = tokio::time::timeout(Duration::from_secs(2), stream_rx.recv())
        .await
        .map_err(|_| "timed out waiting for the decided row")?
        .map_err(|_| "expected a decided row")?;
    let SessionRecord::Permission(PermissionRecord::Decided { decision, .. }) = record else {
        panic!("expected a Decided permission record");
    };
    assert_eq!(
        decision,
        ProtocolPermissionDecision::Deny,
        "a denied card must record the deny decision"
    );
    Ok(())
}

/// A decision timeout returns before the funnel, so it must record the timeout
/// row and no decided row.
#[tokio::test]
async fn test_hook_permission_resolver_submitted_absent_on_timeout() -> Result<()> {
    // -- Setup & Fixtures
    let config = ask_global_with_read_allowed_config()
        .with_decision_timeout(Some(Duration::from_millis(50)));
    let (resolver, bus) = make_interactive(config);
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
    assert!(
        matches!(
            record,
            SessionRecord::Permission(PermissionRecord::Requested { .. })
        ),
        "expected a Requested permission record, got {record:?}"
    );

    // -- Exec
    let decision = tokio::time::timeout(Duration::from_secs(2), resolve_task)
        .await
        .map_err(|_| "resolve never returned after the decision timeout")?
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    assert!(
        matches!(decision, PermissionDecision::Deny { .. }),
        "expected Deny, got {decision:?}"
    );
    let records = permissions(drain(&mut stream_rx));
    assert!(
        records
            .iter()
            .any(|record| matches!(record, PermissionRecord::TimedOut { .. })),
        "the timeout must still record its own row: {records:?}"
    );
    assert!(
        !records
            .iter()
            .any(|record| matches!(record, PermissionRecord::Decided { .. })),
        "a timed-out request recorded no decision: {records:?}"
    );
    Ok(())
}

/// A cancel returns before the funnel, so it must record no decided row.
#[tokio::test]
async fn test_hook_permission_resolver_submitted_absent_on_cancel() -> Result<()> {
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
    assert!(
        matches!(
            record,
            SessionRecord::Permission(PermissionRecord::Requested { .. })
        ),
        "expected a Requested permission record, got {record:?}"
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
    assert!(
        matches!(decision, PermissionDecision::Deny { .. }),
        "expected Deny, got {decision:?}"
    );
    let records = permissions(drain(&mut stream_rx));
    assert!(
        !records
            .iter()
            .any(|record| matches!(record, PermissionRecord::Decided { .. })),
        "a cancelled request recorded no decision: {records:?}"
    );
    Ok(())
}
