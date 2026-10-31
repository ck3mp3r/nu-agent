//! The decision funnel publishes `PermissionDecisionSubmitted`.
//!
//! The funnel is the single point every prompted decision passes through, so
//! one emission there covers the TUI and ACP modes. The timeout and cancel
//! branches return before the funnel, so they must publish nothing.

use std::time::Duration;

use super::*;

/// A submitted decision must reach the bus with the request id, the decision,
/// and the identity of the rule that produced the prompt.
#[tokio::test]
async fn test_hook_permission_resolver_submitted_publishes_decision_and_rule() -> Result<()> {
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
        SystemEvent::PermissionRequested {
            request_id,
            context,
        } => (request_id, context.matched_rule_identity),
        other => panic!("expected PermissionRequested, got {other:?}"),
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
    let event = tokio::time::timeout(Duration::from_secs(2), ui_rx.recv())
        .await
        .map_err(|_| "timed out waiting for PermissionDecisionSubmitted")?
        .map_err(|_| "expected a PermissionDecisionSubmitted event")?;
    match event {
        SystemEvent::PermissionDecisionSubmitted {
            request_id: submitted_id,
            decision: submitted_decision,
            matched_rule_identity,
        } => {
            assert_eq!(
                submitted_id, request_id,
                "the submitted event must name the answered request"
            );
            assert_eq!(
                submitted_decision,
                ProtocolPermissionDecision::AllowOnce,
                "the submitted event must carry the user's decision"
            );
            assert_eq!(
                matched_rule_identity, prompted_rule,
                "the submitted event must carry the rule identity shown on the card"
            );
        }
        other => panic!("expected PermissionDecisionSubmitted, got {other:?}"),
    }
    Ok(())
}

/// A denied decision must be published as `deny`.
#[tokio::test]
async fn test_hook_permission_resolver_submitted_publishes_deny() -> Result<()> {
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
    let event = tokio::time::timeout(Duration::from_secs(2), ui_rx.recv())
        .await
        .map_err(|_| "timed out waiting for PermissionDecisionSubmitted")?
        .map_err(|_| "expected a PermissionDecisionSubmitted event")?;
    match event {
        SystemEvent::PermissionDecisionSubmitted {
            decision: submitted_decision,
            ..
        } => assert_eq!(
            submitted_decision,
            ProtocolPermissionDecision::Deny,
            "a denied card must publish the deny decision"
        ),
        other => panic!("expected PermissionDecisionSubmitted, got {other:?}"),
    }
    Ok(())
}

/// A decision timeout returns before the funnel, so it must publish the timeout
/// event and no submitted event.
#[tokio::test]
async fn test_hook_permission_resolver_submitted_absent_on_timeout() -> Result<()> {
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
    assert!(
        matches!(event, SystemEvent::PermissionRequested { .. }),
        "expected PermissionRequested, got {event:?}"
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
    let events = drain(&mut ui_rx);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, SystemEvent::PermissionDecisionTimedOut { .. })),
        "the timeout must still publish its own event: {events:?}"
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SystemEvent::PermissionDecisionSubmitted { .. })),
        "a timed-out request submitted no decision: {events:?}"
    );
    Ok(())
}

/// A cancel returns before the funnel, so it must publish no submitted event.
#[tokio::test]
async fn test_hook_permission_resolver_submitted_absent_on_cancel() -> Result<()> {
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
    assert!(
        matches!(event, SystemEvent::PermissionRequested { .. }),
        "expected PermissionRequested, got {event:?}"
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
    let events = drain(&mut ui_rx);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SystemEvent::PermissionDecisionSubmitted { .. })),
        "a cancelled request submitted no decision: {events:?}"
    );
    Ok(())
}

// region:    --- Test Support

/// Every event already queued on `rx`.
fn drain(rx: &mut crate::bus::UiEventRx) -> Vec<SystemEvent> {
    let mut events = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events.push(event);
    }
    events
}

// endregion: --- Test Support
