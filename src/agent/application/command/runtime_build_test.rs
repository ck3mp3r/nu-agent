use crate::agent::application::command::runtime_build::{
    build_compaction_params, merge_compaction_configs,
};
use crate::compaction::{CompactionParams, CompactionStrategy};
use crate::config::CompactionConfig;

// --- merge_compaction_configs ---

#[test]
fn merge_cli_overrides_plugin_config() {
    let plugin = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingSummary),
        keep_recent: Some(20),
        token_budget: None,
        proactive_threshold_pct: Some(0.85),
    };

    let cli = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingWindow),
        keep_recent: None,
        token_budget: Some(5000),
        proactive_threshold_pct: None,
    };

    let merged = merge_compaction_configs(Some(&plugin), &cli);

    // CLI wins for strategy
    assert_eq!(merged.strategy, Some(CompactionStrategy::SlidingWindow));
    // Plugin value kept when CLI is None
    assert_eq!(merged.keep_recent, Some(20));
    // CLI provides token_budget
    assert_eq!(merged.token_budget, Some(5000));
    // Plugin value kept
    assert_eq!(merged.proactive_threshold_pct, Some(0.85));
}

#[test]
fn merge_plugin_config_overrides_default() {
    let plugin = CompactionConfig {
        strategy: Some(CompactionStrategy::TokenTruncate),
        keep_recent: None,
        token_budget: Some(8000),
        proactive_threshold_pct: None,
    };

    let cli = CompactionConfig::default(); // all None

    let merged = merge_compaction_configs(Some(&plugin), &cli);

    assert_eq!(merged.strategy, Some(CompactionStrategy::TokenTruncate));
    assert!(merged.keep_recent.is_none()); // neither set
    assert_eq!(merged.token_budget, Some(8000));
}

#[test]
fn merge_default_used_when_no_config() {
    let cli = CompactionConfig::default();
    let merged = merge_compaction_configs(None, &cli);

    assert!(merged.strategy.is_none());
    assert!(merged.keep_recent.is_none());
    assert!(merged.token_budget.is_none());
    assert!(merged.proactive_threshold_pct.is_none());
}

// --- build_compaction_params ---

#[test]
fn build_compaction_params_applies_merged_values() {
    let merged = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingWindow),
        keep_recent: Some(5),
        token_budget: Some(8000),
        proactive_threshold_pct: Some(0.9), // not in SessionConfig
    };

    let config = build_compaction_params(&merged);

    assert_eq!(
        config.compaction_strategy,
        CompactionStrategy::SlidingWindow
    );
    assert_eq!(config.compaction_threshold, 100); // default, no longer overridden
    assert_eq!(config.keep_recent, 5);
    assert_eq!(config.token_budget, Some(8000));
}

#[test]
fn build_compaction_params_uses_defaults_when_none() {
    let merged = CompactionConfig::default(); // all None
    let config = build_compaction_params(&merged);
    let defaults = CompactionParams::default();

    assert_eq!(config.compaction_strategy, defaults.compaction_strategy);
    assert_eq!(config.compaction_threshold, defaults.compaction_threshold);
    assert_eq!(config.keep_recent, defaults.keep_recent);
    assert_eq!(config.token_budget, defaults.token_budget);
}

#[test]
fn build_compaction_params_partial_override() {
    let merged = CompactionConfig {
        strategy: Some(CompactionStrategy::TokenTruncate),
        keep_recent: None,  // use default
        token_budget: None, // use default
        proactive_threshold_pct: None,
    };

    let config = build_compaction_params(&merged);
    let defaults = CompactionParams::default();

    assert_eq!(
        config.compaction_strategy,
        CompactionStrategy::TokenTruncate
    );
    assert_eq!(config.compaction_threshold, defaults.compaction_threshold);
    assert_eq!(config.keep_recent, defaults.keep_recent);
}

// --- Integration: full precedence chain ---

#[test]
fn full_precedence_default_then_plugin_then_cli() {
    // Plugin sets strategy
    let plugin = CompactionConfig {
        strategy: Some(CompactionStrategy::SlidingSummary),
        keep_recent: Some(15),
        token_budget: None,
        proactive_threshold_pct: Some(0.70),
    };

    // CLI overrides strategy only
    let cli = CompactionConfig {
        strategy: Some(CompactionStrategy::TokenTruncate),
        keep_recent: None,
        token_budget: Some(12000),
        proactive_threshold_pct: None,
    };

    let merged = merge_compaction_configs(Some(&plugin), &cli);
    let config = build_compaction_params(&merged);

    // CLI wins for strategy
    assert_eq!(
        config.compaction_strategy,
        CompactionStrategy::TokenTruncate
    );
    // Plugin wins for keep_recent (CLI was None)
    assert_eq!(config.compaction_threshold, 100); // default, no longer overridden
    assert_eq!(config.keep_recent, 15);
    // CLI wins for token_budget
    assert_eq!(config.token_budget, Some(12000));
    // proactive_threshold_pct from plugin (not in CompactionParams, but check merged)
    assert_eq!(merged.proactive_threshold_pct, Some(0.70));
}
