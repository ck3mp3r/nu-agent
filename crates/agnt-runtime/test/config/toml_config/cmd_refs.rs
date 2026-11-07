//! `load_from_with` tests: `cmd:` reference resolution and helper failures.

use super::*;

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
