//! Compaction config serde and validation tests.

use crate::compaction::CompactionStrategy;
use crate::config::CompactionConfig;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn compaction_config_defaults_all_none() {
    let config = CompactionConfig::default();
    assert_eq!(config.strategy, None);
    assert_eq!(config.proactive_threshold_pct, None);
}

#[test]
fn compaction_config_serde_roundtrip() {
    let config = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingSummary),
        proactive_threshold_pct: Some(0.75),
    };

    let json = serde_json::to_string(&config).expect("serialize");
    let deserialized: CompactionConfig = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(config, deserialized);
}

#[test]
fn compaction_config_validate_valid() {
    let config = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingSummary),
        proactive_threshold_pct: Some(0.75),
    };

    assert!(config.validate().is_ok());
}

#[test]
fn compaction_config_validate_pct_out_of_range() -> Result<()> {
    // pct > 1.0
    let config = CompactionConfig {
        proactive_threshold_pct: Some(1.5),
        ..CompactionConfig::default()
    };
    let err = match config.validate() {
        Ok(_) => return Err("pct above 1.0 should fail validation".into()),
        Err(e) => e,
    };
    assert!(err.contains("proactive_threshold_pct"));

    // pct < 0.0
    let config = CompactionConfig {
        proactive_threshold_pct: Some(-0.1),
        ..CompactionConfig::default()
    };
    let err = match config.validate() {
        Ok(_) => return Err("pct below 0.0 should fail validation".into()),
        Err(e) => e,
    };
    assert!(err.contains("proactive_threshold_pct"));
    Ok(())
}
