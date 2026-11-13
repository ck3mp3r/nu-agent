//! `load_from` tests: missing file, minimal, full, partial, and malformed inputs.

use super::*;

#[test]
fn load_returns_default_when_file_missing() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = dir.path().join("nu-agent").join("config.toml");
        let config = load_from(&path)
            .map_err(|e| format!("load should not error when file missing: {e:?}"))?;
        assert_eq!(config, PluginConfig::default());
        Ok(())
    })
}

#[test]
fn load_parses_minimal_config() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_path = write_config(dir, "")?;

        let config = load_from(&config_path).map_err(|e| format!("load should succeed: {e:?}"))?;
        assert_eq!(config, PluginConfig::default());
        Ok(())
    })
}

#[test]
fn load_parses_full_config() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_path = write_config(
            dir,
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
reserve_pct = 35

[agents]
planner_enabled = true
maker_enabled = false
default = "planner"
"#,
        )?;

        let config = load_from(&config_path).map_err(|e| format!("load should succeed: {e:?}"))?;

        // Models
        let default = config.models.get("default").ok_or("default role")?;
        assert_eq!(default.model, "openai/gpt-4");
        assert_eq!(default.temperature, Some(0.7));
        assert_eq!(default.max_tokens, Some(2048));

        let heavy = config.models.get("heavy").ok_or("heavy role")?;
        assert_eq!(heavy.model, "openai/gpt-4-turbo");

        // Providers
        let openai = config.providers.get("openai").ok_or("openai provider")?;
        assert_eq!(openai.name.as_deref(), Some("OpenAI"));
        assert_eq!(openai.base_url.as_deref(), Some("https://api.openai.com"));
        let gpt4 = openai.models.get("gpt-4").ok_or("gpt-4 model")?;
        assert_eq!(gpt4.name.as_deref(), Some("GPT-4"));
        assert_eq!(gpt4.temperature, Some(0.5));
        assert_eq!(gpt4.tool_call, Some(true));

        // Compaction
        let compaction = config.compaction.ok_or("compaction")?;
        assert_eq!(compaction.reserve_pct, Some(35));

        // Agents
        assert!(config.agents.planner_enabled);
        assert!(!config.agents.maker_enabled);
        assert_eq!(config.agents.default, "planner");

        // A2A
        assert_eq!(config.a2a_enabled, Some(true));
        Ok(())
    })
}

#[test]
fn load_parses_every_compaction_policy_field() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_path = write_config(
            dir,
            r#"
[compaction]
reserve_pct = 35
summary_tokens = 6000
snippet_chars = 1500
unknown_window_chars = 200000
keep_recent = 12000
keep_asked = 2000
tracked = [{ tool = "read", argument = "path", set = "files_read" }]

[compaction.clearing]
keep_tokens = 9000
keep_under = 250

[compaction.prompts]
system = "system prompt"
initial = "initial prompt"
update = "update prompt"
format = "format prompt"
in_place = "in place prompt"
notes = "notes prompt"
"#,
        )?;

        // -- Exec
        let config = load_from(&config_path).map_err(|e| format!("load should succeed: {e:?}"))?;

        // -- Check
        let compaction = config.compaction.ok_or("compaction")?;
        assert_eq!(compaction.reserve_pct, Some(35));
        assert_eq!(compaction.summary_tokens, Some(6000));
        assert_eq!(compaction.snippet_chars, Some(1500));
        assert_eq!(compaction.unknown_window_chars, Some(200_000));
        assert_eq!(compaction.keep_recent, Some(12_000));
        assert_eq!(compaction.keep_asked, Some(2_000));

        let clearing = compaction.clearing.ok_or("clearing")?;
        assert_eq!(clearing.keep_tokens, 9000);
        assert_eq!(clearing.keep_under, 250);

        let tracked = compaction.tracked.ok_or("tracked")?;
        assert_eq!(tracked.len(), 1);
        assert_eq!(tracked[0].tool, "read");
        assert_eq!(tracked[0].argument, "path");
        assert_eq!(tracked[0].set, "files_read");

        let prompts = compaction.prompts.ok_or("prompts")?;
        assert_eq!(prompts.system, "system prompt");
        assert_eq!(prompts.initial, "initial prompt");
        assert_eq!(prompts.update, "update prompt");
        assert_eq!(prompts.format, "format prompt");
        assert_eq!(prompts.in_place, "in place prompt");
        assert_eq!(prompts.notes, "notes prompt");
        Ok(())
    })
}

#[test]
fn load_returns_error_on_malformed_toml() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_path = write_config(dir, "this is = not valid [toml")?;

        let err = match load_from(&config_path) {
            Ok(_) => return Err("load should error on malformed toml".into()),
            Err(e) => e,
        };
        assert!(matches!(err, TomlConfigError::Parse(_)));
        Ok(())
    })
}

#[test]
fn load_returns_error_on_wrong_types() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_path = write_config(dir, "a2a_enabled = \"not_a_bool\"")?;

        let err = match load_from(&config_path) {
            Ok(_) => return Err("load should error on wrong types".into()),
            Err(e) => e,
        };
        assert!(matches!(err, TomlConfigError::Parse(_)));
        Ok(())
    })
}

#[test]
fn load_handles_partial_config() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_path = write_config(
            dir,
            r#"
[models.default]
model = "openai/gpt-4"
"#,
        )?;

        let config = load_from(&config_path).map_err(|e| format!("load should succeed: {e:?}"))?;

        // Only models.default set; everything else defaults
        assert_eq!(config.models.len(), 1);
        assert_eq!(config.models["default"].model, "openai/gpt-4");
        assert!(config.providers.is_empty());
        assert!(config.compaction.is_none());
        assert_eq!(config.a2a_enabled, None);
        assert_eq!(config.agents, AgentsConfig::default());
        Ok(())
    })
}

#[test]
fn load_handles_serde_default_annotations() -> Result<()> {
    with_xdg_config_home(|dir| {
        // No sections at all — all #[serde(default)] fields should resolve
        let config_path = write_config(dir, "")?;

        let config = load_from(&config_path).map_err(|e| format!("load should succeed: {e:?}"))?;
        assert_eq!(config, PluginConfig::default());
        Ok(())
    })
}
