//! Compaction config serde and validation tests.

use crate::config::CompactionConfig;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn compaction_config_defaults_and_serde_roundtrip() -> Result<()> {
    // -- Setup & Fixtures
    let default = CompactionConfig::default();
    let config = CompactionConfig {
        reserve_pct: Some(35),
        keep_recent: Some(12_000),
        ..CompactionConfig::default()
    };

    // -- Exec
    let json = serde_json::to_string(&config)?;
    let deserialized: CompactionConfig = serde_json::from_str(&json)?;

    // -- Check
    assert_eq!(default.reserve_pct, None);
    assert_eq!(default.keep_recent, None);
    assert_eq!(config, deserialized);
    Ok(())
}

/// WHEN the `[compaction]` block carries a key the knob replaced, THEN THE parse
/// SHALL reject it: `deny_unknown_fields` fails on a key the struct no longer
/// has rather than ignoring it.
#[test]
fn compaction_config_rejects_the_removed_key() -> Result<()> {
    // -- Exec: the token-reserve key the percent replaced.
    let error = match toml::from_str::<CompactionConfig>("reserve = 1000") {
        Ok(_) => return Err("the removed `reserve` key must be rejected".into()),
        Err(e) => e.to_string(),
    };

    // -- Check
    assert!(
        error.contains("reserve"),
        "the parse error must name the unknown key, got: {error}"
    );
    Ok(())
}

#[test]
fn compaction_config_validate() -> Result<()> {
    // -- Setup & Fixtures
    let cases: Vec<(&str, Option<u32>, bool)> = vec![
        ("valid pct", Some(35), true),
        ("zero", Some(0), true),
        ("hundred", Some(100), true),
        ("pct above 100", Some(101), false),
    ];

    // -- Exec & Check
    for (name, pct, expect_ok) in cases {
        let config = CompactionConfig {
            reserve_pct: pct,
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
            assert!(err.contains("reserve_pct"), "{name}: got {err}");
        }
    }
    Ok(())
}
