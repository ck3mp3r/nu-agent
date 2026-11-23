//! The `[compaction]` table: rig's `CompactionSettings` as a serde passthrough.

use crate::config::CompactionSettings;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The keys this cycle removed. Every one is refused as unknown.
const REMOVED: [&str; 7] = [
    "reserve_pct",
    "summary_tokens",
    "snippet_chars",
    "unknown_window_chars",
    "keep_recent",
    "keep_asked",
    "near_window",
];

/// WHEN `[compaction]` sets one key, THE parse SHALL carry it and leave rig's
/// defaults on the keys the table omits.
#[test]
fn compaction_settings_partial_table_keeps_rig_defaults() -> Result<()> {
    // -- Setup & Fixtures
    let defaults = CompactionSettings::default();

    // -- Exec
    let parsed: CompactionSettings = toml::from_str("reserve_tokens = 40000")?;

    // -- Check
    assert_eq!(parsed.reserve_tokens, 40_000, "the set key is carried");
    assert_eq!(
        parsed.enabled, defaults.enabled,
        "an omitted key keeps rig's default"
    );
    assert_eq!(
        parsed.keep_recent_tokens, defaults.keep_recent_tokens,
        "an omitted key keeps rig's default"
    );
    Ok(())
}

/// WHEN `[compaction]` sets a key this cycle removed, THEN THE parse SHALL refuse
/// it and name the key: rig's table denies unknown fields, so no alias and no
/// silent drop exists.
#[test]
fn compaction_settings_refuse_every_removed_key() -> Result<()> {
    // -- Exec & Check
    for key in REMOVED {
        let body = format!("{key} = 1");
        let error = match toml::from_str::<CompactionSettings>(&body) {
            Ok(_) => return Err(format!("the removed `{key}` key must be refused").into()),
            Err(e) => e.to_string(),
        };
        assert!(
            error.contains(key),
            "the parse error must name `{key}`, got: {error}"
        );
    }
    Ok(())
}
