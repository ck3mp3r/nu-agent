use super::*;

// === Tests: compaction ===

#[test]
fn token_compaction_policy_decisions() {
    // -- Setup & Fixtures
    let exact_cases: &[(u64, f64, Option<u64>, CompactionTriggerDecision)] = &[
        // Fires at the threshold percentage.
        (
            200_000,
            0.80,
            Some(160_000),
            CompactionTriggerDecision::Fire {
                source: CompactionTriggerSource::AutoThreshold,
                reason: "token_usage_80pct_of_80pct_threshold".to_string(),
                strategy: CompactionStrategy::SlidingSummary,
            },
        ),
        // The fire decision carries the configured strategy.
        (
            100_000,
            0.80,
            Some(80_000),
            CompactionTriggerDecision::Fire {
                source: CompactionTriggerSource::AutoThreshold,
                reason: "token_usage_80pct_of_80pct_threshold".to_string(),
                strategy: CompactionStrategy::SlidingSummary,
            },
        ),
        // A custom threshold percentage fires at its own boundary.
        (
            128_000,
            0.90,
            Some(115_200),
            CompactionTriggerDecision::Fire {
                source: CompactionTriggerSource::AutoThreshold,
                reason: "token_usage_90pct_of_90pct_threshold".to_string(),
                strategy: CompactionStrategy::SlidingSummary,
            },
        ),
        // No token data does not fire.
        (
            200_000,
            0.80,
            None,
            CompactionTriggerDecision::NoFire {
                reason: "no_token_data".to_string(),
            },
        ),
        // A zero context window does not fire.
        (
            0,
            0.80,
            Some(100_000),
            CompactionTriggerDecision::NoFire {
                reason: "zero_context_window".to_string(),
            },
        ),
        // Zero tokens do not fire.
        (
            200_000,
            0.80,
            Some(0),
            CompactionTriggerDecision::NoFire {
                reason: "zero_tokens".to_string(),
            },
        ),
    ];

    // -- Exec & Check
    for (window, threshold, tokens, expected) in exact_cases {
        let policy = TokenCompactionPolicy::new(*window, *threshold);
        assert_eq!(
            &policy.evaluate(*tokens),
            expected,
            "window={window} threshold={threshold} tokens={tokens:?}"
        );
    }

    // Below the threshold does not fire.
    let policy = TokenCompactionPolicy::new(200_000, 0.80);
    assert!(matches!(
        policy.evaluate(Some(159_999)),
        CompactionTriggerDecision::NoFire { .. }
    ));

    // Regression: Some(1) with a large context window stays below threshold.
    assert!(matches!(
        policy.evaluate(Some(1)),
        CompactionTriggerDecision::NoFire { reason: ref r } if r.contains("below_threshold")
    ));

    // The custom threshold boundary: 115,199 stays below.
    let policy = TokenCompactionPolicy::new(128_000, 0.90);
    assert!(matches!(
        policy.evaluate(Some(115_199)),
        CompactionTriggerDecision::NoFire { .. }
    ));

    // Token-based policy is stateless — the same input gives the same result.
    let policy = TokenCompactionPolicy::new(200_000, 0.80);
    let first = policy.evaluate(Some(160_000));
    let second = policy.evaluate(Some(160_000));
    assert_eq!(first, second);
}
