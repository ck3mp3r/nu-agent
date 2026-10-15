use super::*;

// ============================================================
// Strategy tests
// ============================================================

/// The default strategy is `SlidingSummary`; legacy aliases normalize to it;
/// and `CompactionParams` round-trips through serde.
#[test]
fn compaction_strategy_and_params() -> core::result::Result<(), Box<dyn std::error::Error>> {
    // -- Case 1: the default is SlidingSummary
    let cfg = CompactionParams::default();
    assert_eq!(cfg.compaction_strategy, CompactionStrategy::SlidingSummary);
    assert_eq!(cfg.compaction_strategy.as_str(), "sliding_summary");

    // -- Case 2: legacy strategy values normalize to SlidingSummary
    for alias in [
        "truncate",
        "sliding",
        "summarize",
        "sliding_window",
        "token_truncate",
        "sliding_summary",
    ] {
        let parsed: CompactionStrategy = serde_json::from_str(&format!("\"{alias}\""))?;
        assert_eq!(
            parsed,
            CompactionStrategy::SlidingSummary,
            "alias {alias} must normalize to SlidingSummary"
        );
    }

    // -- Case 3: CompactionParams round-trips
    let cfg = CompactionParams {
        compaction_strategy: CompactionStrategy::SlidingSummary,
    };
    let json = serde_json::to_string(&cfg)?;
    assert!(json.contains("sliding_summary"));
    let decoded: CompactionParams = serde_json::from_str(&json)?;
    assert_eq!(
        decoded.compaction_strategy,
        CompactionStrategy::SlidingSummary
    );
    Ok(())
}
