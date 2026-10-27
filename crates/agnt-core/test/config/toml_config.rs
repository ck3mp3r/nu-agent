use super::*;
use crate::compaction::CompactionStrategy;
use crate::config::AgentsConfig;
use crate::config::cmd_refs::{CmdRunError, CmdRunner};
use crate::utils::env_map::EnvMap;
use tempfile::TempDir;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// Helper to run a test with a controlled XDG_CONFIG_HOME pointing to a temp dir.
fn with_xdg_config_home<F>(test: F) -> Result<()>
where
    F: FnOnce(&TempDir) -> Result<()>,
{
    let dir = TempDir::new().map_err(|e| format!("failed to create temp dir: {e}"))?;
    test(&dir)
}

/// Build an [`EnvMap`] pointing `XDG_CONFIG_HOME` at `dir`.
fn env_for(dir: &TempDir) -> EnvMap {
    EnvMap::from([(
        "XDG_CONFIG_HOME".to_string(),
        dir.path().to_string_lossy().to_string(),
    )])
}

/// Answers `gh auth token` and `op read 'OpenAI API Key'`; fails everything else.
struct StubRunner;

impl CmdRunner for StubRunner {
    fn run(&self, program: &str, args: &[&str]) -> core::result::Result<String, CmdRunError> {
        match (program, args) {
            ("gh", ["auth", "token"]) => Ok("ghp_from_stub\n".to_string()),
            ("op", ["read", "OpenAI API Key"]) => Ok("sk-op\n".to_string()),
            _ => Err(CmdRunError::Io(std::io::Error::other(format!(
                "unexpected command: {program} {args:?}"
            )))),
        }
    }
}

/// Fails every command with a non-zero exit.
struct FailingRunner;

impl CmdRunner for FailingRunner {
    fn run(&self, _program: &str, _args: &[&str]) -> core::result::Result<String, CmdRunError> {
        Err(CmdRunError::Io(std::io::Error::other("stub failure")))
    }
}

/// Write `contents` to `<dir>/nu-agent/config.toml` and return the path.
fn write_config(dir: &TempDir, contents: &str) -> Result<std::path::PathBuf> {
    let config_dir = dir.path().join("nu-agent");
    std::fs::create_dir_all(&config_dir).expect("create config dir");
    let path = config_dir.join("config.toml");
    std::fs::write(&path, contents).expect("write config");
    Ok(path)
}

#[test]
fn config_path_returns_xdg_path() -> Result<()> {
    with_xdg_config_home(|dir| {
        let expected = dir.path().join("nu-agent").join("config.toml");
        let path = config_path_with(&env_for(dir))
            .map_err(|e| format!("config_path should succeed: {e:?}"))?;
        assert_eq!(path, expected);
        Ok(())
    })
}

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
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(config_dir.join("config.toml"), "").expect("write config");

        let config = load_from(&config_dir.join("config.toml"))
            .map_err(|e| format!("load should succeed: {e:?}"))?;
        assert_eq!(config, PluginConfig::default());
        Ok(())
    })
}

// region:    --- Modules

#[path = "toml_config/hooks/mod.rs"]
mod hooks;

#[path = "toml_config/pricing.rs"]
mod pricing;

// endregion: --- Modules

#[test]
fn load_from_with_resolves_cmd_reference_in_mcp_env() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[mcp.gh]
command = "npx"

[mcp.gh.env]
GITHUB_TOKEN = "cmd:gh auth token"
"#,
        )?;

        let config = load_from_with(&path, &StubRunner)
            .map_err(|e| format!("load should succeed: {e:?}"))?;

        let token = config
            .mcp
            .as_ref()
            .and_then(|mcp| mcp.get("gh"))
            .and_then(|gh| gh.get("env"))
            .and_then(|env| env.get("GITHUB_TOKEN"))
            .and_then(toml::Value::as_str);
        assert_eq!(token, Some("ghp_from_stub"));
        Ok(())
    })
}

#[test]
fn load_from_with_resolves_cmd_reference_in_api_key() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[providers.openai]
api_key = "cmd:op read 'OpenAI API Key'"
"#,
        )?;

        let config = load_from_with(&path, &StubRunner)
            .map_err(|e| format!("load should succeed: {e:?}"))?;

        let openai = config.providers.get("openai").ok_or("openai provider")?;
        assert_eq!(openai.api_key.as_deref(), Some("sk-op"));
        Ok(())
    })
}

#[test]
fn load_from_with_reports_cmd_helper_failure() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[mcp.gh.env]
GITHUB_TOKEN = "cmd:gh auth token"
"#,
        )?;

        let err = match load_from_with(&path, &FailingRunner) {
            Ok(_) => return Err("a failing cmd: reference must error".into()),
            Err(e) => e,
        };

        let TomlConfigError::CmdHelper(message) = err else {
            return Err("expected CmdHelper error".into());
        };
        assert!(
            message.contains("mcp.gh.env.GITHUB_TOKEN"),
            "message must name the key path: {message}"
        );
        assert!(
            message.contains("gh auth token"),
            "message must name the command: {message}"
        );
        Ok(())
    })
}

#[test]
fn load_from_with_leaves_config_without_cmd_references_unchanged() -> Result<()> {
    with_xdg_config_home(|dir| {
        let path = write_config(
            dir,
            r#"
[models.default]
model = "openai/gpt-4"

[providers.openai]
api_key = "store:openai"

[mcp.gh.env]
GITHUB_TOKEN = "env:GITHUB_TOKEN"
"#,
        )?;

        let config = load_from_with(&path, &FailingRunner)
            .map_err(|e| format!("load should succeed: {e:?}"))?;

        assert_eq!(config.models["default"].model, "openai/gpt-4");
        let openai = config.providers.get("openai").ok_or("openai provider")?;
        assert_eq!(openai.api_key.as_deref(), Some("store:openai"));
        let token = config
            .mcp
            .as_ref()
            .and_then(|mcp| mcp.get("gh"))
            .and_then(|gh| gh.get("env"))
            .and_then(|env| env.get("GITHUB_TOKEN"))
            .and_then(toml::Value::as_str);
        assert_eq!(token, Some("env:GITHUB_TOKEN"));
        Ok(())
    })
}

#[test]
fn load_parses_full_config() -> Result<()> {
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

        let config = load_from(&config_dir.join("config.toml"))
            .map_err(|e| format!("load should succeed: {e:?}"))?;

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
        Ok(())
    })
}

#[test]
fn load_returns_error_on_malformed_toml() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(config_dir.join("config.toml"), "this is = not valid [toml")
            .expect("write config");

        let err = match load_from(&config_dir.join("config.toml")) {
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
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("config.toml"),
            "a2a_enabled = \"not_a_bool\"",
        )
        .expect("write config");

        let err = match load_from(&config_dir.join("config.toml")) {
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

        let config = load_from(&config_dir.join("config.toml"))
            .map_err(|e| format!("load should succeed: {e:?}"))?;

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
fn load_parses_theme_key() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("config.toml"),
            "theme = \"catppuccin-latte\"",
        )
        .expect("write config");

        let config = load_from(&config_dir.join("config.toml"))
            .map_err(|e| format!("load should succeed: {e:?}"))?;
        assert_eq!(config.theme.as_deref(), Some("catppuccin-latte"));
        Ok(())
    })
}

#[test]
fn load_keeps_invalid_theme_raw() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(config_dir.join("config.toml"), "theme = \"bogus\"").expect("write config");

        let config = load_from(&config_dir.join("config.toml"))
            .map_err(|e| format!("load should succeed: {e:?}"))?;
        assert_eq!(config.theme.as_deref(), Some("bogus"));
        Ok(())
    })
}

#[test]
fn load_rejects_raw_api_key() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[providers.openai]
api_key = "sk-raw-literal"
"#,
        )
        .expect("write config");

        let err = match load_from(&config_dir.join("config.toml")) {
            Ok(_) => return Err("load should reject a raw api_key".into()),
            Err(e) => e,
        };
        let TomlConfigError::InvalidApiKey(message) = err else {
            return Err("expected InvalidApiKey error".into());
        };
        assert!(
            message.contains("providers.openai"),
            "message must name the provider: {message}"
        );
        assert!(
            message.contains("agent provider auth login openai"),
            "message must suggest the login command: {message}"
        );
        assert!(
            message.contains("cmd:"),
            "message must list the cmd: reference: {message}"
        );
        Ok(())
    })
}

#[test]
fn load_accepts_store_reference() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[providers.openai]
api_key = "store:openai"
"#,
        )
        .expect("write config");

        let config = load_from(&config_dir.join("config.toml"))
            .map_err(|e| format!("load should succeed: {e:?}"))?;
        let openai = config.providers.get("openai").ok_or("openai provider")?;
        assert_eq!(openai.api_key.as_deref(), Some("store:openai"));
        Ok(())
    })
}

#[test]
fn load_accepts_env_reference() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        std::fs::write(
            config_dir.join("config.toml"),
            r#"
[providers.openai]
api_key = "env:MY_CUSTOM_KEY"
"#,
        )
        .expect("write config");

        let config = load_from(&config_dir.join("config.toml"))
            .map_err(|e| format!("load should succeed: {e:?}"))?;
        let openai = config.providers.get("openai").ok_or("openai provider")?;
        assert_eq!(openai.api_key.as_deref(), Some("env:MY_CUSTOM_KEY"));
        Ok(())
    })
}

#[test]
fn load_handles_serde_default_annotations() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_dir = dir.path().join("nu-agent");
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        // No sections at all — all #[serde(default)] fields should resolve
        std::fs::write(config_dir.join("config.toml"), "").expect("write config");

        let config = load_from(&config_dir.join("config.toml"))
            .map_err(|e| format!("load should succeed: {e:?}"))?;
        assert_eq!(config, PluginConfig::default());
        Ok(())
    })
}
