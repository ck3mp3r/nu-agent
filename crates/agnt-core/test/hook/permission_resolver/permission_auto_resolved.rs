//! Both resolvers publish `PermissionAutoResolved` when policy decides without a prompt.
//!
//! The event marks the policy-resolved-without-prompting moment. It carries the
//! matched rule identity, the action (`"allow"` or `"deny"`), and a reason that
//! is `Some` only on deny. The Ask path owns its own events and must not publish
//! this one.

use super::*;

/// The stderr policy resolver publishes the auto-resolved event on an explicit
/// allow rule, with no reason.
#[tokio::test]
async fn test_hook_permission_resolver_permission_auto_resolved_policy_allow() -> Result<()> {
    // -- Setup & Fixtures
    let resolver = make_policy(ask_global_with_read_allowed_config());
    let bus = Bus::default();
    let mut ui_rx = bus.ui_event().subscribe();

    // -- Exec
    let decision = resolver.resolve(ALLOW_TOOL, "{}", None, &bus).await;

    // -- Check
    assert_eq!(decision, PermissionDecision::Allow);
    let events = drain(&mut ui_rx);
    let resolved = auto_resolved(&events)?;
    assert_eq!(resolved.matched_rule_identity, "tool:read");
    assert_eq!(resolved.action.as_str(), "allow");
    assert_eq!(
        resolved.reason, None,
        "an allow carries no reason: {events:?}"
    );
    Ok(())
}

/// The stderr policy resolver publishes the auto-resolved event on a policy
/// deny, with the rule-derived denial reason.
#[tokio::test]
async fn test_hook_permission_resolver_permission_auto_resolved_policy_deny() -> Result<()> {
    // -- Setup & Fixtures
    let resolver = make_policy(deny_all_toml_config());
    let bus = Bus::default();
    let mut ui_rx = bus.ui_event().subscribe();

    // -- Exec
    let decision = resolver.resolve(ASK_TOOL, "{}", None, &bus).await;

    // -- Check
    let deny_reason = match decision {
        PermissionDecision::Deny { reason } => reason,
        other => panic!("expected Deny, got {other:?}"),
    };
    let events = drain(&mut ui_rx);
    let resolved = auto_resolved(&events)?;
    assert_eq!(resolved.matched_rule_identity, "global:*");
    assert_eq!(resolved.action.as_str(), "deny");
    assert_eq!(
        resolved.reason.as_deref(),
        Some(deny_reason.as_str()),
        "the deny reason must match the returned denial: {events:?}"
    );
    Ok(())
}

/// The interactive resolver's auto-allow branch publishes the event with no
/// reason and no prompt.
#[tokio::test]
async fn test_hook_permission_resolver_permission_auto_resolved_interactive_auto_allow()
-> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut ui_rx = bus.ui_event().subscribe();

    // -- Exec
    let decision = resolver.resolve(ALLOW_TOOL, "{}", None, &bus).await;

    // -- Check
    assert_eq!(decision, PermissionDecision::Allow);
    let events = drain(&mut ui_rx);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SystemEvent::PermissionRequested { .. })),
        "an auto-allowed call must not prompt: {events:?}"
    );
    let resolved = auto_resolved(&events)?;
    assert_eq!(resolved.matched_rule_identity, "tool:read");
    assert_eq!(resolved.action.as_str(), "allow");
    assert_eq!(
        resolved.reason, None,
        "an allow carries no reason: {events:?}"
    );
    Ok(())
}

/// The interactive resolver's auto-deny branch publishes the event with the
/// rule-derived denial reason and no prompt.
#[tokio::test]
async fn test_hook_permission_resolver_permission_auto_resolved_interactive_auto_deny() -> Result<()>
{
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(deny_all_toml_config());
    let mut ui_rx = bus.ui_event().subscribe();

    // -- Exec
    let decision = resolver.resolve(ASK_TOOL, "{}", None, &bus).await;

    // -- Check
    let deny_reason = match decision {
        PermissionDecision::Deny { reason } => reason,
        other => panic!("expected Deny, got {other:?}"),
    };
    let events = drain(&mut ui_rx);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SystemEvent::PermissionRequested { .. })),
        "an auto-denied call must not prompt: {events:?}"
    );
    let resolved = auto_resolved(&events)?;
    assert_eq!(resolved.matched_rule_identity, "global:*");
    assert_eq!(resolved.action.as_str(), "deny");
    assert_eq!(
        resolved.reason.as_deref(),
        Some(deny_reason.as_str()),
        "the deny reason must match the returned denial: {events:?}"
    );
    Ok(())
}

/// The Ask path publishes the prompt events and never the auto-resolved event.
#[tokio::test]
async fn test_hook_permission_resolver_permission_auto_resolved_absent_on_ask() -> Result<()> {
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

    let event = tokio::time::timeout(std::time::Duration::from_secs(2), ui_rx.recv())
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
        ProtocolPermissionDecision::AllowOnce,
    );
    let decision = tokio::time::timeout(std::time::Duration::from_secs(2), resolve_task)
        .await
        .map_err(|_| "resolve never returned after the decision")?
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    assert_eq!(decision, PermissionDecision::Allow);
    let events = drain(&mut ui_rx);
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, SystemEvent::PermissionAutoResolved { .. })),
        "the Ask path owns its own events and must not publish auto-resolved: {events:?}"
    );
    Ok(())
}

// region:    --- Test Support

/// The single auto-resolved event on `events`.
fn auto_resolved(events: &[SystemEvent]) -> Result<AutoResolved> {
    let mut found = events.iter().filter_map(|event| match event {
        SystemEvent::PermissionAutoResolved {
            matched_rule_identity,
            action,
            reason,
        } => Some(AutoResolved {
            matched_rule_identity: matched_rule_identity.clone(),
            action: *action,
            reason: reason.clone(),
        }),
        _ => None,
    });
    let resolved = found
        .next()
        .ok_or_else(|| format!("expected a PermissionAutoResolved event: {events:?}"))?;
    assert!(
        found.next().is_none(),
        "exactly one auto-resolved event per resolved call: {events:?}"
    );
    Ok(resolved)
}

/// The three payload fields of one auto-resolved event.
struct AutoResolved {
    matched_rule_identity: String,
    action: AutoResolutionAction,
    reason: Option<String>,
}

// endregion: --- Test Support
