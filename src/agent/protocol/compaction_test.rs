use super::compaction::{
    CompactionTriggerDecision, CompactionTriggerPolicy, CompactionTriggerSource,
    CompactionTriggerState, TwoTierCompactionPolicy,
};
use crate::session::CompactionStrategy;

#[test]
fn policy_below_threshold_does_not_fire() {
    let policy = TwoTierCompactionPolicy::new(100, 0, 0);
    let mut state = CompactionTriggerState::default();

    let decision = policy.evaluate(Some(94), &mut state);

    assert_eq!(
        decision,
        CompactionTriggerDecision::NoFire {
            reason: "below_lower_bound".to_string()
        }
    );
}

#[test]
fn policy_with_zero_hysteresis_refires_on_sustained_usage() {
    let policy = TwoTierCompactionPolicy::new(100, 2, 0);
    let mut state = CompactionTriggerState::default();

    let at_lower_bound = policy.evaluate(Some(98), &mut state);
    let second = policy.evaluate(Some(100), &mut state);

    assert_eq!(
        at_lower_bound,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }
    );
    assert_eq!(
        second,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "sustained_high_usage".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }
    );
}

#[test]
fn policy_refires_when_disarmed_but_above_threshold() {
    let policy = TwoTierCompactionPolicy::new(100, 1, 2);
    let mut state = CompactionTriggerState::default();

    let first = policy.evaluate(Some(99), &mut state);
    let near_threshold = policy.evaluate(Some(100), &mut state);

    assert_eq!(
        first,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }
    );
    assert_eq!(
        near_threshold,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "sustained_high_usage".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }
    );
}

#[test]
fn policy_rearms_after_usage_drops_below_rearm_bound() {
    let policy = TwoTierCompactionPolicy::new(100, 2, 3);
    let mut state = CompactionTriggerState::default();

    let fire = policy.evaluate(Some(98), &mut state);
    let still_disarmed = policy.evaluate(Some(96), &mut state);
    let rearmed_no_fire = policy.evaluate(Some(95), &mut state);
    let fire_again = policy.evaluate(Some(100), &mut state);

    assert_eq!(
        fire,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }
    );
    assert_eq!(
        still_disarmed,
        CompactionTriggerDecision::FallbackFire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "approaching_threshold".to_string(),
            strategies: vec![CompactionStrategy::SlidingWindow],
        }
    );
    assert_eq!(
        rearmed_no_fire,
        CompactionTriggerDecision::FallbackFire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "approaching_threshold".to_string(),
            strategies: vec![CompactionStrategy::SlidingWindow],
        }
    );
    assert_eq!(
        fire_again,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }
    );
}

#[test]
fn policy_unknown_signal_is_deterministic_no_fire() {
    let policy = TwoTierCompactionPolicy::new(100, 0, 0);
    let mut state = CompactionTriggerState::default();

    let decision = policy.evaluate(None, &mut state);

    assert_eq!(
        decision,
        CompactionTriggerDecision::NoFire {
            reason: "signal_unavailable".to_string(),
        }
    );
}

#[test]
fn sustained_high_usage_retriggers_compaction() {
    // threshold=100, tolerance=10, hysteresis_margin=5
    // lower_bound=90, rearm_bound=85
    let policy = TwoTierCompactionPolicy::new(100, 10, 5);
    let mut state = CompactionTriggerState::default();

    // Fire at 95 (armed, 95 >= lower_bound 90)
    let first_fire = policy.evaluate(Some(95), &mut state);
    assert_eq!(
        first_fire,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }
    );

    // Still at 92 — above rearm_bound(85) AND above lower_bound(90)
    // Should force re-fire with "sustained_high_usage"
    let sustained = policy.evaluate(Some(92), &mut state);
    assert_eq!(
        sustained,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "sustained_high_usage".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }
    );
}

#[test]
fn normal_cycle_still_works() {
    // threshold=100, tolerance=10, hysteresis_margin=5
    // lower_bound=90, rearm_bound=85
    let policy = TwoTierCompactionPolicy::new(100, 10, 5);
    let mut state = CompactionTriggerState::default();

    // Fire at 95
    let first_fire = policy.evaluate(Some(95), &mut state);
    assert_eq!(
        first_fire,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }
    );

    // Drop to 80 — below rearm_bound(85), rearms
    let rearmed = policy.evaluate(Some(80), &mut state);
    assert_eq!(
        rearmed,
        CompactionTriggerDecision::NoFire {
            reason: "rearmed".to_string(),
        }
    );

    // Back up to 95 — armed again, fires normally
    let second_fire = policy.evaluate(Some(95), &mut state);
    assert_eq!(
        second_fire,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }
    );
}

#[test]
fn usage_drops_below_rearm_bound_rearms_without_firing() {
    // threshold=100, tolerance=10, hysteresis_margin=5
    // lower_bound=90, rearm_bound=85
    let policy = TwoTierCompactionPolicy::new(100, 10, 5);
    let mut state = CompactionTriggerState::default();

    // Fire at 95
    let first_fire = policy.evaluate(Some(95), &mut state);
    assert_eq!(
        first_fire,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }
    );

    // Drop to 80 — below rearm_bound(85) AND below lower_bound(90)
    // Should rearm without firing
    let rearmed = policy.evaluate(Some(80), &mut state);
    assert_eq!(
        rearmed,
        CompactionTriggerDecision::NoFire {
            reason: "rearmed".to_string(),
        }
    );
    assert!(state.armed());
}

#[test]
fn two_tier_fire_includes_primary_strategy() {
    let policy = TwoTierCompactionPolicy::new(100, 10, 5);
    let mut state = CompactionTriggerState::default();

    let decision = policy.evaluate(Some(95), &mut state);

    assert_eq!(
        decision,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }
    );
}

#[test]
fn two_tier_fallback_at_95_percent() {
    // threshold=100, tolerance=0, hysteresis_margin=0
    // lower_bound=100, fallback_bound=95
    let policy = TwoTierCompactionPolicy::new(100, 0, 0);
    let mut state = CompactionTriggerState::default();

    // Fire at 100 to disarm
    let fire = policy.evaluate(Some(100), &mut state);
    assert_eq!(
        fire,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
            strategy: CompactionStrategy::SlidingSummary,
        }
    );
    assert!(!state.armed());

    // Evaluate at 96 — disarmed, above rearm_bound(100), below lower_bound(100)
    // But 96 >= fallback_bound(95), so FallbackFire
    let fallback = policy.evaluate(Some(96), &mut state);
    assert_eq!(
        fallback,
        CompactionTriggerDecision::FallbackFire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "approaching_threshold".to_string(),
            strategies: vec![CompactionStrategy::SlidingWindow],
        }
    );
}
