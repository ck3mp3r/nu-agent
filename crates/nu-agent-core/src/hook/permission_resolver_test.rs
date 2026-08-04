use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::mpsc;

use crate::protocol::event::{PermissionDecision as ProtocolPermissionDecision, UiEvent};
use crate::tools::authz::{
    PermissionAction, PermissionDecision as AuthzPermissionDecision, PermissionRuleMatch,
    PermissionsConfig, SessionGrantCache,
};
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;

use super::{
    AsyncPermissionResolver, InteractivePermissionResolver, PermissionDecision,
    PolicyPermissionResolver,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build a resolver pair for interactive tests: resolver + event receiver.
fn make_interactive(
    permissions: PermissionsConfig,
) -> (
    InteractivePermissionResolver,
    mpsc::UnboundedReceiver<UiEvent>,
    mpsc::UnboundedSender<UiEvent>,
) {
    let (ui_tx, ui_rx) = mpsc::unbounded_channel();
    let resolver = InteractivePermissionResolver {
        pending: Arc::new(StdMutex::new(HashMap::new())),
        permissions: Arc::new(permissions),
        session_grants: Arc::new(StdMutex::new(SessionGrantCache::default())),
        closure_registry: Arc::new(ClosureRegistry::new()),
        mcp_registry: Arc::new(McpToolRegistry::from_names::<[&str; 0], &str>([])),
    };
    (resolver, ui_rx, ui_tx)
}

fn make_policy(permissions: PermissionsConfig) -> PolicyPermissionResolver {
    PolicyPermissionResolver {
        permissions: Arc::new(permissions),
        session_grants: Arc::new(StdMutex::new(SessionGrantCache::default())),
        closure_registry: Arc::new(ClosureRegistry::new()),
        mcp_registry: Arc::new(McpToolRegistry::from_names::<[&str; 0], &str>([])),
    }
}

/// `safe_defaults(false)` → global Deny.
/// All tools that don't match an explicit allow rule will be Deny.
fn deny_all_config() -> PermissionsConfig {
    PermissionsConfig::safe_defaults(false)
}

/// `safe_defaults(true)` → global Ask, but "read" is explicitly Allow.
/// Use "read" as the "explicit allow" tool in tests.
fn ask_global_with_read_allowed_config() -> PermissionsConfig {
    PermissionsConfig::safe_defaults(true)
}

/// A tool name that is not in the explicit allow list of safe_defaults(true)
/// so it falls through to the global Ask action.
const ASK_TOOL: &str = "some_mcp_tool";

/// A tool that is explicitly allowed in `safe_defaults(true)`.
const ALLOW_TOOL: &str = "read";

// ---------------------------------------------------------------------------
// PolicyPermissionResolver tests
// ---------------------------------------------------------------------------

/// Test 1: PolicyPermissionResolver + explicit allow config → Allow.
#[tokio::test]
async fn policy_resolver_explicit_allow_returns_allow() {
    let resolver = make_policy(ask_global_with_read_allowed_config());
    let decision = resolver.resolve(ALLOW_TOOL, "{}", None, None).await;
    assert_eq!(decision, PermissionDecision::Allow);
}

/// Test 2: PolicyPermissionResolver + explicit deny config → Deny.
#[tokio::test]
async fn policy_resolver_explicit_deny_returns_deny() {
    let resolver = make_policy(deny_all_config());
    // "some_mcp_tool" not in the allow list, and global is Deny
    let decision = resolver.resolve(ASK_TOOL, "{}", None, None).await;
    assert_eq!(decision, PermissionDecision::Deny);
}

/// Test 3: PolicyPermissionResolver + ask config → Deny (NoOpAskHook fires, no interaction).
#[tokio::test]
async fn policy_resolver_ask_config_returns_deny_without_interaction() {
    let resolver = make_policy(ask_global_with_read_allowed_config());
    // ASK_TOOL falls through to global Ask → NoOpAskHook returns Deny
    let decision = resolver.resolve(ASK_TOOL, "{}", None, None).await;
    assert_eq!(decision, PermissionDecision::Deny);
}

// ---------------------------------------------------------------------------
// InteractivePermissionResolver tests
// ---------------------------------------------------------------------------

/// Test 4: InteractivePermissionResolver + explicit allow config → Allow, no PermissionRequested.
#[tokio::test]
async fn interactive_resolver_explicit_allow_returns_allow_no_event() {
    let (resolver, mut ui_rx, ui_tx) = make_interactive(ask_global_with_read_allowed_config());
    let decision = resolver.resolve(ALLOW_TOOL, "{}", None, Some(ui_tx)).await;
    assert_eq!(decision, PermissionDecision::Allow);
    // No PermissionRequested event should have been emitted
    assert!(
        ui_rx.try_recv().is_err(),
        "Expected no PermissionRequested event for explicitly allowed tool"
    );
}

/// Test 5: InteractivePermissionResolver + explicit deny config → Deny, no PermissionRequested.
#[tokio::test]
async fn interactive_resolver_explicit_deny_returns_deny_no_event() {
    let (resolver, mut ui_rx, ui_tx) = make_interactive(deny_all_config());
    let decision = resolver.resolve(ASK_TOOL, "{}", None, Some(ui_tx)).await;
    assert_eq!(decision, PermissionDecision::Deny);
    // No PermissionRequested event should have been emitted
    assert!(
        ui_rx.try_recv().is_err(),
        "Expected no PermissionRequested event for explicitly denied tool"
    );
}

/// Test 6: InteractivePermissionResolver + ask config → PermissionRequested sent,
/// submit_decision(request_id, Allow) → resolve returns Allow.
#[tokio::test]
async fn interactive_resolver_ask_config_submit_allow_returns_allow() {
    let (resolver, mut ui_rx, ui_tx) = make_interactive(ask_global_with_read_allowed_config());

    // Clone the resolver so we can call submit_decision from a separate task
    let resolver_clone = resolver.clone();

    // Spawn resolve() — it will block waiting for submit_decision
    let resolve_fut =
        tokio::spawn(async move { resolver.resolve(ASK_TOOL, "{}", None, Some(ui_tx)).await });

    // Wait for the PermissionRequested event
    let event = ui_rx
        .recv()
        .await
        .expect("Expected PermissionRequested event");
    let request_id = match event {
        UiEvent::PermissionRequested { request_id, .. } => request_id,
        other => panic!("Expected PermissionRequested, got {:?}", other),
    };

    // Submit Allow decision
    resolver_clone.submit_decision(&request_id, ProtocolPermissionDecision::AllowOnce);

    // Verify resolve() returned Allow
    let decision = resolve_fut.await.expect("resolve task panicked");
    assert_eq!(decision, PermissionDecision::Allow);
}

/// Test 7: InteractivePermissionResolver + ask config → submit_decision(request_id, Deny) → Deny.
#[tokio::test]
async fn interactive_resolver_ask_config_submit_deny_returns_deny() {
    let (resolver, mut ui_rx, ui_tx) = make_interactive(ask_global_with_read_allowed_config());

    let resolver_clone = resolver.clone();

    let resolve_fut =
        tokio::spawn(async move { resolver.resolve(ASK_TOOL, "{}", None, Some(ui_tx)).await });

    // Wait for the PermissionRequested event
    let event = ui_rx
        .recv()
        .await
        .expect("Expected PermissionRequested event");
    let request_id = match event {
        UiEvent::PermissionRequested { request_id, .. } => request_id,
        other => panic!("Expected PermissionRequested, got {:?}", other),
    };

    // Submit Deny decision
    resolver_clone.submit_decision(&request_id, ProtocolPermissionDecision::Deny);

    let decision = resolve_fut.await.expect("resolve task panicked");
    assert_eq!(decision, PermissionDecision::Deny);
}

/// Test 8: InteractivePermissionResolver + AllowAlways decision writes grant to cache,
/// and a subsequent resolve() for the same tool returns Allow WITHOUT emitting another
/// PermissionRequested event.
#[tokio::test]
async fn interactive_allow_always_writes_grant_and_auto_allows_subsequent_call() {
    let (resolver, mut ui_rx, ui_tx) = make_interactive(ask_global_with_read_allowed_config());

    // Clone the resolver so we can call submit_decision from a separate task
    let resolver_clone = resolver.clone();

    // --- First call: should trigger PermissionRequested ---
    let resolve_fut = tokio::spawn({
        let resolver = resolver.clone();
        let ui_tx = ui_tx.clone();
        async move { resolver.resolve(ASK_TOOL, "{}", None, Some(ui_tx)).await }
    });

    // Wait for the PermissionRequested event
    let event = ui_rx
        .recv()
        .await
        .expect("Expected PermissionRequested event");
    let request_id = match event {
        UiEvent::PermissionRequested { request_id, .. } => request_id,
        other => panic!("Expected PermissionRequested, got {:?}", other),
    };

    // Submit AllowAlways decision — this should write a grant into session_grants
    resolver_clone.submit_decision(&request_id, ProtocolPermissionDecision::AllowAlways);

    // First resolve() must return Allow
    let decision = resolve_fut.await.expect("resolve task panicked");
    assert_eq!(
        decision,
        PermissionDecision::Allow,
        "AllowAlways should return Allow"
    );

    // --- Second call: must return Allow WITHOUT emitting another PermissionRequested ---
    let decision2 = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        resolver.resolve(ASK_TOOL, "{}", None, Some(ui_tx)),
    )
    .await
    .expect("second resolve timed out — AllowAlways grant not persisted to session cache");
    assert_eq!(
        decision2,
        PermissionDecision::Allow,
        "second call should hit session grant and return Allow without asking"
    );

    // No new PermissionRequested event should have been emitted for the second call
    assert!(
        ui_rx.try_recv().is_err(),
        "Expected no PermissionRequested event for second call after AllowAlways grant"
    );
}

// ---------------------------------------------------------------------------
// Regression test: session_grants Arc is shared across resolver instances
// ---------------------------------------------------------------------------

/// Regression test: proves that two resolvers sharing the same `session_grants`
/// Arc see each other's writes.
///
/// Previously, `runtime.rs` cloned `SessionGrantCache` into a **fresh**
/// `Arc::new(Mutex::new(...))` on every turn, so any grants written during
/// a turn were silently dropped when the Arc went out of scope.
///
/// After the fix, `PermissionState` owns the single `Arc<Mutex<SessionGrantCache>>`
/// and hands out Arc-clones via `session_grants_arc()`.  This test verifies the
/// sharing property directly: a grant inserted through one Arc-clone is visible
/// through another.
#[tokio::test]
async fn session_grant_arc_is_shared_across_resolver_instances() {
    // Shared cache — the same Arc that PermissionState now owns.
    let session_grants = Arc::new(StdMutex::new(SessionGrantCache::default()));

    // Build two resolvers that share the same Arc (as runtime.rs now does).
    let resolver1 = PolicyPermissionResolver {
        permissions: Arc::new(ask_global_with_read_allowed_config()),
        session_grants: Arc::clone(&session_grants),
        closure_registry: Arc::new(ClosureRegistry::new()),
        mcp_registry: Arc::new(McpToolRegistry::from_names::<[&str; 0], &str>([])),
    };
    let resolver2 = PolicyPermissionResolver {
        permissions: Arc::new(ask_global_with_read_allowed_config()),
        session_grants: Arc::clone(&session_grants),
        closure_registry: Arc::new(ClosureRegistry::new()),
        mcp_registry: Arc::new(McpToolRegistry::from_names::<[&str; 0], &str>([])),
    };

    // Before any grant: resolver1 sees Deny (global Ask → NoOpAskHook → Deny).
    let before = resolver1.resolve(ASK_TOOL, "{}", None, None).await;
    assert_eq!(
        before,
        PermissionDecision::Deny,
        "expected Deny before any session grant is inserted"
    );

    // Simulate what apply_ask_choice(AllowAlways) would write into the cache.
    // The evaluate() for ASK_TOOL ("some_mcp_tool") against safe_defaults(true)
    // matches the global rule: identity="global:*", source="unknown" (not in any
    // registry), mode=None, target_field=None.
    {
        let mut cache = session_grants.lock().expect("test lock");
        let synthetic_decision = AuthzPermissionDecision {
            action: PermissionAction::Ask,
            matched_rule: PermissionRuleMatch {
                identity: "global:*".to_string(),
                scope: "global",
                target_field: None,
                pattern: "*".to_string(),
                action: PermissionAction::Ask,
            },
            diagnostics: Vec::new(),
        };
        // insert_allow_always writes the grant keyed by (identity, tool_name, source, mode, target_field).
        cache.insert_allow_always(
            &synthetic_decision,
            ASK_TOOL,
            "unknown",
            &serde_json::json!({}),
        );
    }

    // resolver2 must now return Allow — the cache hit via apply_session_grant_override
    // upgrades Ask → Allow before the NoOpAskHook is even consulted.
    let after = resolver2.resolve(ASK_TOOL, "{}", None, None).await;
    assert_eq!(
        after,
        PermissionDecision::Allow,
        "expected Allow after AllowAlways grant was inserted into the shared cache"
    );
}

// AllowAlways write path fixed in commit 30a4611: apply_ask_choice is now called
// after the TUI oneshot resolves in InteractivePermissionResolver::resolve().
// Regression test: interactive_allow_always_writes_grant_and_auto_allows_subsequent_call

// Test: interactive_allow_always_uses_captured_decision_not_re_evaluation
//
// The intent of this test is to verify that the `matched_rule_identity` used to write
// the AllowAlways grant (the cache key) is the same one that was shown to the user
// during the sentinel run — i.e. that no second `permissions.evaluate()` call can
// produce a different result.
//
// Why a meaningful assertion is NOT achievable with the current test infrastructure:
//
// 1. `PermissionsConfig` is deterministic and pure: for a given (tool_name, args) input
//    it always returns the same `PermissionDecision`. There is no observable difference
//    between "captured first call" and "re-evaluated second call" — both would produce
//    identical `matched_rule` values.
//
// 2. There is no test hook exposing which `matched_rule_identity` was used to write a
//    specific `SessionGrantCache` entry. The cache stores opaque keys; there is no
//    `get_last_written_identity()` API.
//
// 3. Constructing a `PermissionsConfig` with two overlapping rules where evaluation
//    order matters (the scenario where re-evaluation could return a different rule) is
//    not possible via the public API — `safe_defaults` and `from_rules` provide no
//    ordering guarantees observable from outside the module.
//
// The correctness guarantee is therefore structural: `choose_with_sink` now sets
// `captured_auth_decision = Some(decision.clone())` and the async block consumes it
// directly via `if let Some(auth_decision) = capture_hook.captured_auth_decision`.
// The second `permissions.evaluate()` call has been removed. No runtime assertion can
// add more confidence than is already provided by:
//   - the type system (the field is `Option<PermissionDecision>` — None is impossible
//     when `was_called == true` because both fields are set together)
//   - interactive_allow_always_writes_grant_and_auto_allows_subsequent_call (which
//     confirms the grant IS written correctly and the cache hit works end-to-end)
//
// A redundant test is omitted to avoid false assurance.

#[test]
fn policy_resolver_satisfies_async_permission_resolver_bounds() {
    fn assert_bounds<T: AsyncPermissionResolver>() {}
    assert_bounds::<PolicyPermissionResolver>();
}

#[test]
fn interactive_resolver_satisfies_async_permission_resolver_bounds() {
    fn assert_bounds<T: AsyncPermissionResolver>() {}
    assert_bounds::<InteractivePermissionResolver>();
}
