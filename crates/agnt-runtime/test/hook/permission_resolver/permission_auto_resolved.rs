//! Both resolvers record `PermissionRecord::AutoResolved` when policy decides
//! without a prompt.
//!
//! The row marks the policy-resolved-without-prompting moment. It carries the
//! matched rule identity, the action (`AutoResolution::Allow`/`Deny`), and a
//! reason that is `Some` only on deny. The Ask path owns its own rows and must
//! not record this one.

use super::*;

/// The stderr policy resolver records the auto-resolved row on an explicit
/// allow rule, with no reason.
#[tokio::test]
async fn test_hook_permission_resolver_permission_auto_resolved_policy_allow() -> Result<()> {
    // -- Setup & Fixtures
    let resolver = make_policy(ask_global_with_read_allowed_config());
    let bus = Bus::default();
    let mut stream_rx = bus.stream().subscribe();

    // -- Exec
    let decision = resolver
        .resolve(
            ALLOW_TOOL,
            "{}",
            None,
            bus.stream(),
            bus.facts(),
            bus.cancel(),
        )
        .await;

    // -- Check
    assert_eq!(decision, PermissionDecision::Allow);
    let records = permissions(drain(&mut stream_rx));
    let resolved = auto_resolved(&records)?;
    assert_eq!(resolved.rule_identity, "tool:read");
    assert_eq!(resolved.action, AutoResolution::Allow);
    assert_eq!(
        resolved.reason, None,
        "an allow carries no reason: {records:?}"
    );
    Ok(())
}

/// The stderr policy resolver records the auto-resolved row on a policy deny,
/// with the rule-derived denial reason.
#[tokio::test]
async fn test_hook_permission_resolver_permission_auto_resolved_policy_deny() -> Result<()> {
    // -- Setup & Fixtures
    let resolver = make_policy(deny_all_toml_config());
    let bus = Bus::default();
    let mut stream_rx = bus.stream().subscribe();

    // -- Exec
    let decision = resolver
        .resolve(
            ASK_TOOL,
            "{}",
            None,
            bus.stream(),
            bus.facts(),
            bus.cancel(),
        )
        .await;

    // -- Check
    let deny_reason = match decision {
        PermissionDecision::Deny { reason } => reason,
        other => panic!("expected Deny, got {other:?}"),
    };
    let records = permissions(drain(&mut stream_rx));
    let resolved = auto_resolved(&records)?;
    assert_eq!(resolved.rule_identity, "global:*");
    assert_eq!(resolved.action, AutoResolution::Deny);
    assert_eq!(
        resolved.reason.as_deref(),
        Some(deny_reason.as_str()),
        "the deny reason must match the returned denial: {records:?}"
    );
    Ok(())
}

/// The interactive resolver's auto-allow branch records the row with no reason
/// and no prompt.
#[tokio::test]
async fn test_hook_permission_resolver_permission_auto_resolved_interactive_auto_allow()
-> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut stream_rx = bus.stream().subscribe();

    // -- Exec
    let decision = resolver
        .resolve(
            ALLOW_TOOL,
            "{}",
            None,
            bus.stream(),
            bus.facts(),
            bus.cancel(),
        )
        .await;

    // -- Check
    assert_eq!(decision, PermissionDecision::Allow);
    let records = permissions(drain(&mut stream_rx));
    assert!(
        !records
            .iter()
            .any(|record| matches!(record, PermissionRecord::Requested { .. })),
        "an auto-allowed call must not prompt: {records:?}"
    );
    let resolved = auto_resolved(&records)?;
    assert_eq!(resolved.rule_identity, "tool:read");
    assert_eq!(resolved.action, AutoResolution::Allow);
    assert_eq!(
        resolved.reason, None,
        "an allow carries no reason: {records:?}"
    );
    Ok(())
}

/// The interactive resolver's auto-deny branch records the row with the
/// rule-derived denial reason and no prompt.
#[tokio::test]
async fn test_hook_permission_resolver_permission_auto_resolved_interactive_auto_deny() -> Result<()>
{
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(deny_all_toml_config());
    let mut stream_rx = bus.stream().subscribe();

    // -- Exec
    let decision = resolver
        .resolve(
            ASK_TOOL,
            "{}",
            None,
            bus.stream(),
            bus.facts(),
            bus.cancel(),
        )
        .await;

    // -- Check
    let deny_reason = match decision {
        PermissionDecision::Deny { reason } => reason,
        other => panic!("expected Deny, got {other:?}"),
    };
    let records = permissions(drain(&mut stream_rx));
    assert!(
        !records
            .iter()
            .any(|record| matches!(record, PermissionRecord::Requested { .. })),
        "an auto-denied call must not prompt: {records:?}"
    );
    let resolved = auto_resolved(&records)?;
    assert_eq!(resolved.rule_identity, "global:*");
    assert_eq!(resolved.action, AutoResolution::Deny);
    assert_eq!(
        resolved.reason.as_deref(),
        Some(deny_reason.as_str()),
        "the deny reason must match the returned denial: {records:?}"
    );
    Ok(())
}

/// The Ask path records the prompt rows and never the auto-resolved row.
#[tokio::test]
async fn test_hook_permission_resolver_permission_auto_resolved_absent_on_ask() -> Result<()> {
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

    let record = tokio::time::timeout(std::time::Duration::from_secs(2), stream_rx.recv())
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
    let decision = tokio::time::timeout(std::time::Duration::from_secs(2), resolve_task)
        .await
        .map_err(|_| "resolve never returned after the decision")?
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    assert_eq!(decision, PermissionDecision::Allow);
    let records = permissions(drain(&mut stream_rx));
    assert!(
        !records
            .iter()
            .any(|record| matches!(record, PermissionRecord::AutoResolved { .. })),
        "the Ask path owns its own rows and must not record auto-resolved: {records:?}"
    );
    Ok(())
}

// region:    --- Test Support

/// The single auto-resolved payload on `records`.
fn auto_resolved(records: &[PermissionRecord]) -> Result<AutoResolved> {
    let mut found = records.iter().filter_map(|record| match record {
        PermissionRecord::AutoResolved {
            rule_identity,
            action,
            reason,
            ..
        } => Some(AutoResolved {
            rule_identity: rule_identity.clone(),
            action: *action,
            reason: reason.clone(),
        }),
        _ => None,
    });
    let resolved = found
        .next()
        .ok_or_else(|| format!("expected an AutoResolved permission record: {records:?}"))?;
    assert!(
        found.next().is_none(),
        "exactly one auto-resolved row per resolved call: {records:?}"
    );
    Ok(resolved)
}

/// The three payload fields of one auto-resolved row.
struct AutoResolved {
    rule_identity: String,
    action: AutoResolution,
    reason: Option<String>,
}

// endregion: --- Test Support
