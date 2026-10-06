use super::*;

// ============================================================
// Strategy tests
// ============================================================

#[test]
fn compaction_strategy_defaults_to_sliding_summary_only() {
    let cfg = CompactionParams::default();
    assert_eq!(cfg.compaction_strategy, CompactionStrategy::SlidingSummary);
    assert_eq!(cfg.compaction_strategy.as_str(), "sliding_summary");
}

#[test]
fn legacy_strategy_values_normalize_to_sliding_summary() {
    let truncate: CompactionStrategy =
        serde_json::from_str("\"truncate\"").expect("truncate alias");
    let sliding: CompactionStrategy = serde_json::from_str("\"sliding\"").expect("sliding alias");
    let summarize: CompactionStrategy =
        serde_json::from_str("\"summarize\"").expect("summarize alias");
    let sliding_window: CompactionStrategy =
        serde_json::from_str("\"sliding_window\"").expect("sliding_window alias");
    let token_truncate: CompactionStrategy =
        serde_json::from_str("\"token_truncate\"").expect("token_truncate alias");
    let canonical: CompactionStrategy =
        serde_json::from_str("\"sliding_summary\"").expect("canonical");

    assert_eq!(truncate, CompactionStrategy::SlidingSummary);
    assert_eq!(sliding, CompactionStrategy::SlidingSummary);
    assert_eq!(summarize, CompactionStrategy::SlidingSummary);
    assert_eq!(sliding_window, CompactionStrategy::SlidingSummary);
    assert_eq!(token_truncate, CompactionStrategy::SlidingSummary);
    assert_eq!(canonical, CompactionStrategy::SlidingSummary);
}

#[test]
fn compaction_params_roundtrip_preserves_sliding_summary_mode() {
    let cfg = CompactionParams {
        compaction_strategy: CompactionStrategy::SlidingSummary,
    };
    let json = serde_json::to_string(&cfg).expect("serialize");
    assert!(json.contains("sliding_summary"));

    let decoded: CompactionParams = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        decoded.compaction_strategy,
        CompactionStrategy::SlidingSummary
    );
}
