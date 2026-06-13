use super::*;

#[test]
fn compaction_strategy_defaults_to_sliding_summary_only() {
    let cfg = CompactionParams::default();
    assert_eq!(
        cfg.compaction_strategy,
        CompactionStrategy::SlidingSummary
    );
    assert_eq!(cfg.compaction_strategy.as_str(), "sliding_summary");
}

#[test]
fn sliding_window_deserializes_from_canonical_name() {
    let strategy: CompactionStrategy =
        serde_json::from_str("\"sliding_window\"").expect("sliding_window canonical");
    assert_eq!(strategy, CompactionStrategy::SlidingWindow);
    assert_eq!(strategy.as_str(), "sliding_window");
}

#[test]
fn sliding_window_roundtrip_preserves_name() {
    let strategy = CompactionStrategy::SlidingWindow;
    let json = serde_json::to_string(&strategy).expect("serialize");
    assert_eq!(json, "\"sliding_window\"");
    let decoded: CompactionStrategy =
        serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded, CompactionStrategy::SlidingWindow);
}

#[test]
fn compaction_params_roundtrip_preserves_sliding_window_mode() {
    let cfg = CompactionParams {
        compaction_threshold: 50,
        compaction_strategy: CompactionStrategy::SlidingWindow,
        keep_recent: 5,
        token_budget: None,
    };
    let json = serde_json::to_string(&cfg).expect("serialize");
    assert!(json.contains("sliding_window"));

    let decoded: CompactionParams = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        decoded.compaction_strategy,
        CompactionStrategy::SlidingWindow
    );
}

#[test]
fn legacy_strategy_values_normalize_to_sliding_summary() {
    let truncate: CompactionStrategy =
        serde_json::from_str("\"truncate\"").expect("truncate alias");
    let sliding: CompactionStrategy =
        serde_json::from_str("\"sliding\"").expect("sliding alias");
    let summarize: CompactionStrategy =
        serde_json::from_str("\"summarize\"").expect("summarize alias");
    let canonical: CompactionStrategy =
        serde_json::from_str("\"sliding_summary\"").expect("canonical");

    assert_eq!(truncate, CompactionStrategy::SlidingSummary);
    assert_eq!(sliding, CompactionStrategy::SlidingSummary);
    assert_eq!(
        summarize,
        CompactionStrategy::SlidingSummary
    );
    assert_eq!(
        canonical,
        CompactionStrategy::SlidingSummary
    );
}

#[test]
fn compaction_params_roundtrip_preserves_sliding_summary_mode() {
    let cfg = CompactionParams {
        compaction_threshold: 7,
        compaction_strategy: CompactionStrategy::SlidingSummary,
        keep_recent: 3,
        token_budget: None,
    };
    let json = serde_json::to_string(&cfg).expect("serialize");
    assert!(json.contains("sliding_summary"));

    let decoded: CompactionParams = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        decoded.compaction_strategy,
        CompactionStrategy::SlidingSummary
    );
}
