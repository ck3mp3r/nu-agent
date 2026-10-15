use super::*;

// ---------------------------------------------------------------------------
// Denial reason enrichment
// ---------------------------------------------------------------------------

/// A policy deny decision carries a reason that starts with "Permission denied"
/// and identifies the matched rule by identity and scope.
#[tokio::test]
async fn policy_resolver_deny_reason_carries_rule_identity_and_scope() {
    // -- Setup & Fixtures
    let resolver = make_policy(deny_all_config());
    let bus = Bus::default();

    // -- Exec
    let decision = resolver.resolve(ASK_TOOL, "{}", None, &bus).await;

    // -- Check
    let reason = match decision {
        PermissionDecision::Deny { reason } => reason,
        other => panic!("expected Deny, got {other:?}"),
    };
    assert!(
        reason.starts_with("Permission denied"),
        "denial reason must start with 'Permission denied', got: {reason}"
    );
    assert!(
        reason.contains("global:*"),
        "denial reason must carry the matched rule identity, got: {reason}"
    );
    assert!(
        reason.contains("global"),
        "denial reason must carry the matched rule scope, got: {reason}"
    );
}

/// A user-denied (Ask → Deny) decision also carries the rule identity of the
/// rule that triggered the permission prompt.
#[tokio::test]
async fn interactive_resolver_ask_deny_reason_carries_rule_identity() -> Result<()> {
    // -- Setup & Fixtures
    let (resolver, bus) = make_interactive(ask_global_with_read_allowed_config());
    let mut permission_rx = bus.ui_event().subscribe();
    let resolver_clone = resolver.clone();

    let resolve_fut = tokio::spawn({
        let bus = bus.clone();
        async move { resolver.resolve(ASK_TOOL, "{}", None, &bus).await }
    });

    // -- Exec
    let event = permission_rx
        .recv()
        .await
        .map_err(|_| "Expected PermissionRequested event")?;
    let request_id = match event {
        UiEvent::PermissionRequested { request_id, .. } => request_id,
        other => panic!("Expected PermissionRequested, got {other:?}"),
    };
    resolver_clone.submit_decision(&request_id, ProtocolPermissionDecision::Deny);

    let decision = resolve_fut
        .await
        .map_err(|e| format!("resolve task panicked: {e:?}"))?;

    // -- Check
    let reason = match decision {
        PermissionDecision::Deny { reason } => reason,
        other => panic!("expected Deny, got {other:?}"),
    };
    assert!(
        reason.starts_with("Permission denied"),
        "denial reason must start with 'Permission denied', got: {reason}"
    );
    assert!(
        reason.contains("global:*"),
        "denial reason must carry the matched rule identity shown to the user, got: {reason}"
    );
    Ok(())
}

/// An allow decision carries no reason payload: `Allow` stays a unit variant.
#[tokio::test]
async fn policy_resolver_allow_decision_has_no_reason_payload() {
    // -- Setup & Fixtures
    let resolver = make_policy(ask_global_with_read_allowed_config());
    let bus = Bus::default();

    // -- Exec
    let decision = resolver.resolve(ALLOW_TOOL, "{}", None, &bus).await;

    // -- Check
    assert!(
        matches!(decision, PermissionDecision::Allow),
        "expected the bare Allow unit variant, got: {decision:?}"
    );
}
