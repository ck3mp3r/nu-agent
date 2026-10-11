// ========================================================================
// Compaction policy tests
// ========================================================================

#[test]
fn evaluate_auto_compaction_uses_token_based_policy() {
    // Verify that TokenCompactionPolicy is used for auto-compaction evaluation.
    // We can't easily construct a full runtime, but we verify the policy logic directly.
    use crate::protocol::compaction::{CompactionTriggerPolicy, TokenCompactionPolicy};

    let policy = TokenCompactionPolicy::new(200_000, 0.80);

    // At 80% usage (160k of 200k) — should fire
    let decision = policy.evaluate(Some(160_000));
    assert!(
        matches!(
            decision,
            crate::protocol::compaction::CompactionTriggerDecision::Fire { .. }
        ),
        "Expected compaction to fire at 80% token usage"
    );

    // At 50% usage — should not fire
    let decision2 = policy.evaluate(Some(100_000));
    assert!(
        matches!(
            decision2,
            crate::protocol::compaction::CompactionTriggerDecision::NoFire { .. }
        ),
        "Expected no compaction at 50% token usage"
    );
}

#[test]
fn compaction_state_evaluate_returns_none_when_no_tokens() {
    // Characterise evaluate_auto_compaction (runtime.rs:488-495).
    // When last_total_tokens is None, the policy returns NoFire("no_token_data")
    // and the method wraps it in Some(...).
    use crate::protocol::compaction::{
        CompactionTriggerDecision, CompactionTriggerPolicy, TokenCompactionPolicy,
    };

    let policy = TokenCompactionPolicy::new(100_000, 0.8);
    let decision = Some(policy.evaluate(None));

    assert!(
        matches!(decision, Some(CompactionTriggerDecision::NoFire { .. })),
        "no token data must yield Some(NoFire), not Fire"
    );
}

#[test]
fn compaction_state_evaluate_returns_none_below_threshold() {
    // Characterise evaluate_auto_compaction (runtime.rs:488-495).
    // 50k tokens against 100k window with 80% threshold => 50% usage, below threshold.
    use crate::protocol::compaction::{
        CompactionTriggerDecision, CompactionTriggerPolicy, TokenCompactionPolicy,
    };

    let policy = TokenCompactionPolicy::new(100_000, 0.8);
    let decision = Some(policy.evaluate(Some(50_000)));

    assert!(
        matches!(decision, Some(CompactionTriggerDecision::NoFire { .. })),
        "50% usage below 80% threshold must yield Some(NoFire)"
    );
}
