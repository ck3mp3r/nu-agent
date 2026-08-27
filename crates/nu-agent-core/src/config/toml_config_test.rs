use super::*;
use crate::compaction::CompactionStrategy;
use crate::config::AgentsConfig;
use serial_test::serial;
use std::env;
use tempfile::TempDir;

/// Helper to run a test with a controlled XDG_CONFIG_HOME pointing to a temp dir.
fn with_xdg_config_home<F>(test: F)
where
    F: FnOnce(&TempDir),
{
    let dir = TempDir::new().expect("failed to create temp dir");
    unsafe {
        env::set_var("XDG_CONFIG_HOME", dir.path());
    }
    test(&dir);
    unsafe {
        env::remove_var("XDG_CONFIG_HOME");
    }
}

#[test]
#[serial]
fn config_path_returns_xdg_path() {
    with_xdg_config_home(|dir| {
        let expected = dir.path().join("nu-agent").join("config.toml");
        let path = config_path().expect("config_path should succeed");
        assert_eq!(path, expected);
    });
}

#[test]
#[serial]
fn load_returns_default_when_file_missing() {
    with_xdg_config_home(|_| {
        let config = load().expect("load should not error when file missing");
        assert_eq!(config, PluginConfig::default());
    });
}

#[test]
#[serial]
fn load_parses_minimal_config() {
    with_xdg_config_home(|dir| {
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(config_dir.join("config.toml"), "").expect("write config");

        let config = load().expect("load should succeed");
        assert_eq!(config, PluginConfig::default());
    });
}

#[test]
#[serial]
fn load_parses_full_config() {
    with_xdg_config_home(|dir| {
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
a2a_enabled = true

[models.default]
model = "openai/gpt-4"
temperature = 0.7
max_tokens = 2048

[models.heavy]
model = "openai/gpt-4-turbo"

[providers.openai]
name = "OpenAI"
base_url = "https://api.openai.com"

[providers.openai.models.gpt-4]
name = "GPT-4"
temperature = 0.5
tool_call = true

[compaction]
strategy = "sliding_summary"
proactive_threshold_pct = 0.75

[agents]
planner_enabled = true
maker_enabled = false
default = "planner"
"#,
        )
        .expect("write config");

        let config = load().expect("load should succeed");

        // Models
        let default = config.models.get("default").expect("default role");
        assert_eq!(default.model, "openai/gpt-4");
        assert_eq!(default.temperature, Some(0.7));
        assert_eq!(default.max_tokens, Some(2048));

        let heavy = config.models.get("heavy").expect("heavy role");
        assert_eq!(heavy.model, "openai/gpt-4-turbo");

        // Providers
        let openai = config.providers.get("openai").expect("openai provider");
        assert_eq!(openai.name.as_deref(), Some("OpenAI"));
        assert_eq!(openai.base_url.as_deref(), Some("https://api.openai.com"));
        let gpt4 = openai.models.get("gpt-4").expect("gpt-4 model");
        assert_eq!(gpt4.name.as_deref(), Some("GPT-4"));
        assert_eq!(gpt4.temperature, Some(0.5));
        assert_eq!(gpt4.tool_call, Some(true));

        // Compaction
        let compaction = config.compaction.expect("compaction");
        assert_eq!(
            compaction.strategy,
            Some(CompactionStrategy::SlidingSummary)
        );
        assert_eq!(compaction.proactive_threshold_pct, Some(0.75));

        // Agents
        assert!(config.agents.planner_enabled);
        assert!(!config.agents.maker_enabled);
        assert_eq!(config.agents.default, "planner");

        // A2A
        assert_eq!(config.a2a_enabled, Some(true));
    });
}

#[test]
#[serial]
fn load_returns_error_on_malformed_toml() {
    with_xdg_config_home(|dir| {
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(config_dir.join("config.toml"), "this is = not valid [toml")
            .expect("write config");

        let err = load().expect_err("load should error on malformed toml");
        assert!(matches!(err, TomlConfigError::Parse(_)));
    });
}

#[test]
#[serial]
fn load_returns_error_on_wrong_types() {
    with_xdg_config_home(|dir| {
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("config.toml"),
            "a2a_enabled = \"not_a_bool\"",
        )
        .expect("write config");

        let err = load().expect_err("load should error on wrong types");
        assert!(matches!(err, TomlConfigError::Parse(_)));
    });
}

#[test]
#[serial]
fn load_handles_partial_config() {
    with_xdg_config_home(|dir| {
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[models.default]
model = "openai/gpt-4"
"#,
        )
        .expect("write config");

        let config = load().expect("load should succeed");

        // Only models.default set; everything else defaults
        assert_eq!(config.models.len(), 1);
        assert_eq!(config.models["default"].model, "openai/gpt-4");
        assert!(config.providers.is_empty());
        assert!(config.compaction.is_none());
        assert_eq!(config.a2a_enabled, None);
        assert_eq!(config.agents, AgentsConfig::default());
    });
}

#[test]
#[serial]
fn load_handles_serde_default_annotations() {
    with_xdg_config_home(|dir| {
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        // No sections at all — all #[serde(default)] fields should resolve
        std::fs::write(config_dir.join("config.toml"), "").expect("write config");

        let config = load().expect("load should succeed");
        assert_eq!(config, PluginConfig::default());
    });
}
