use super::compaction::{
    CompactionTriggerDecision, CompactionTriggerPolicy, CompactionTriggerSource,
    CompactionTriggerState, ThresholdCompactionPolicy,
};

#[test]
fn policy_below_threshold_does_not_fire() {
    let policy = ThresholdCompactionPolicy::new(100, 0, 0);
    let mut state = CompactionTriggerState::default();

    let decision = policy.evaluate(Some(99), &mut state);

    assert_eq!(
        decision,
        CompactionTriggerDecision::NoFire {
            reason: "below_lower_bound".to_string()
        }
    );
}

#[test]
fn policy_at_or_within_tolerance_fires_once() {
    let policy = ThresholdCompactionPolicy::new(100, 2, 0);
    let mut state = CompactionTriggerState::default();

    let at_lower_bound = policy.evaluate(Some(98), &mut state);
    let second = policy.evaluate(Some(100), &mut state);

    assert_eq!(
        at_lower_bound,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
        }
    );
    assert_eq!(
        second,
        CompactionTriggerDecision::NoFire {
            reason: "disarmed".to_string(),
        }
    );
}

#[test]
fn policy_does_not_refire_while_disarmed_near_boundary() {
    let policy = ThresholdCompactionPolicy::new(100, 1, 2);
    let mut state = CompactionTriggerState::default();

    let first = policy.evaluate(Some(99), &mut state);
    let near_threshold = policy.evaluate(Some(100), &mut state);

    assert_eq!(
        first,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
        }
    );
    assert_eq!(
        near_threshold,
        CompactionTriggerDecision::NoFire {
            reason: "disarmed".to_string(),
        }
    );
}

#[test]
fn policy_rearms_after_usage_drops_below_rearm_bound() {
    let policy = ThresholdCompactionPolicy::new(100, 2, 3);
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
        }
    );
    assert_eq!(
        still_disarmed,
        CompactionTriggerDecision::NoFire {
            reason: "disarmed".to_string(),
        }
    );
    assert_eq!(
        rearmed_no_fire,
        CompactionTriggerDecision::NoFire {
            reason: "rearmed".to_string(),
        }
    );
    assert_eq!(
        fire_again,
        CompactionTriggerDecision::Fire {
            source: CompactionTriggerSource::AutoThreshold,
            reason: "threshold_reached".to_string(),
        }
    );
}

#[test]
fn policy_unknown_signal_is_deterministic_no_fire() {
    let policy = ThresholdCompactionPolicy::new(100, 0, 0);
    let mut state = CompactionTriggerState::default();

    let decision = policy.evaluate(None, &mut state);

    assert_eq!(
        decision,
        CompactionTriggerDecision::NoFire {
            reason: "signal_unavailable".to_string(),
        }
    );
}
