//! Compaction config serde and validation tests.

use crate::compaction::CompactionStrategy;
use crate::config::CompactionConfig;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn compaction_config_defaults_and_serde_roundtrip() -> Result<()> {
    // -- Setup & Fixtures
    let default = CompactionConfig::default();
    let config = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingSummary),
        proactive_threshold_pct: Some(0.75),
    };

    // -- Exec
    let json = serde_json::to_string(&config)?;
    let deserialized: CompactionConfig = serde_json::from_str(&json)?;

    // -- Check
    assert_eq!(default.strategy, None);
    assert_eq!(default.proactive_threshold_pct, None);
    assert_eq!(config, deserialized);
    Ok(())
}

#[test]
fn compaction_config_validate() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, Option<f64>, bool)> = vec![
        ("valid pct", Some(0.75), true),
        ("pct above 1.0", Some(1.5), false),
        ("pct below 0.0", Some(-0.1), false),
    ];

    // -- Exec & Check
    for (name, pct, expect_ok) in cases {
        let config = CompactionConfig {
            proactive_threshold_pct: pct,
            ..CompactionConfig::default()
        };
        let result = config.validate();
        if expect_ok {
            assert!(result.is_ok(), "{name}: {result:?}");
        } else {
            let err = match result {
                Ok(_) => return Err(format!("{name} should fail validation").into()),
                Err(e) => e,
            };
            assert!(err.contains("proactive_threshold_pct"), "{name}: got {err}");
        }
    }
    Ok(())
}
