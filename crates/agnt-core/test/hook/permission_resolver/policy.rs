use super::*;

// ---------------------------------------------------------------------------
// PolicyPermissionResolver tests
// ---------------------------------------------------------------------------

/// Test 1: PolicyPermissionResolver + explicit allow config → Allow.
#[tokio::test]
async fn policy_resolver_explicit_allow_returns_allow() {
    let resolver = make_policy(ask_global_with_read_allowed_config());
    let bus = Bus::default();
    let decision = resolver.resolve(ALLOW_TOOL, "{}", None, &bus).await;
    assert_eq!(decision, PermissionDecision::Allow);
}

/// Test 2: PolicyPermissionResolver + explicit deny config → Deny.
#[tokio::test]
async fn policy_resolver_explicit_deny_returns_deny() {
    let resolver = make_policy(deny_all_config());
    let bus = Bus::default();
    // "some_mcp_tool" not in the allow list, and global is Deny
    let decision = resolver.resolve(ASK_TOOL, "{}", None, &bus).await;
    assert!(
        matches!(decision, PermissionDecision::Deny { .. }),
        "expected Deny, got {decision:?}"
    );
}

/// Test 3: PolicyPermissionResolver + ask config → Deny (NoOpAskHook fires, no interaction).
#[tokio::test]
async fn policy_resolver_ask_config_returns_deny_without_interaction() {
    let resolver = make_policy(ask_global_with_read_allowed_config());
    let bus = Bus::default();
    // ASK_TOOL falls through to global Ask → NoOpAskHook returns Deny
    let decision = resolver.resolve(ASK_TOOL, "{}", None, &bus).await;
    assert!(
        matches!(decision, PermissionDecision::Deny { .. }),
        "expected Deny, got {decision:?}"
    );
}
