use super::compaction::{
    CompactionTriggerDecision, CompactionTriggerPolicy, CompactionTriggerSource,
    TokenCompactionPolicy,
};
use crate::session::CompactionStrategy;

#[test]
fn fires_at_threshold_percentage() {
    let policy = TokenCompactionPolicy::new(200_000, 0.80, CompactionStrategy::SlidingSummary);
    let decision = policy.evaluate(Some(160_000));
    assert!(matches!(decision, CompactionTriggerDecision::Fire { .. }));
}

#[test]
fn does_not_fire_below_threshold() {
    let policy = TokenCompactionPolicy::new(200_000, 0.80, CompactionStrategy::SlidingSummary);
    let decision = policy.evaluate(Some(159_999));
    assert!(matches!(decision, CompactionTriggerDecision::NoFire { .. }));
}

#[test]
fn does_not_fire_when_no_token_data() {
    let policy = TokenCompactionPolicy::new(200_000, 0.80, CompactionStrategy::SlidingSummary);
    let decision = policy.evaluate(None);
    assert_eq!(
        decision,
        CompactionTriggerDecision::NoFire {
            reason: "no_token_data".to_string()
        }
    );
}

#[test]
fn does_not_fire_with_zero_context_window() {
    let policy = TokenCompactionPolicy::new(0, 0.80, CompactionStrategy::SlidingSummary);
    let decision = policy.evaluate(Some(100_000));
    assert_eq!(
        decision,
        CompactionTriggerDecision::NoFire {
            reason: "zero_context_window".to_string()
        }
    );
}

#[test]
fn respects_custom_threshold_percentage() {
    let policy = TokenCompactionPolicy::new(128_000, 0.90, CompactionStrategy::SlidingSummary);
    // 90% of 128k = 115,200
    let below = policy.evaluate(Some(115_199));
    let at = policy.evaluate(Some(115_200));
    assert!(matches!(below, CompactionTriggerDecision::NoFire { .. }));
    assert!(matches!(at, CompactionTriggerDecision::Fire { .. }));
}

#[test]
fn fire_includes_configured_strategy() {
    let policy = TokenCompactionPolicy::new(100_000, 0.80, CompactionStrategy::SlidingWindow);
    let decision = policy.evaluate(Some(80_000));
    assert_eq!(
        decision,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "token_usage_80pct_of_80pct_threshold".to_string(),
            strategy: CompactionStrategy::SlidingWindow,
        }
    );
}

#[test]
fn no_state_needed_between_evaluations() {
    // Token-based policy is stateless — calling evaluate twice with same input gives same result
    let policy = TokenCompactionPolicy::new(200_000, 0.80, CompactionStrategy::SlidingSummary);
    let first = policy.evaluate(Some(160_000));
    let second = policy.evaluate(Some(160_000));
    assert_eq!(first, second);
}
