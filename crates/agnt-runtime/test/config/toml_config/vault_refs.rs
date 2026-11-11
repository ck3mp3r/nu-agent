//! `api_key` vault-reference policy: raw literals are rejected, references pass.

use super::*;

#[test]
fn load_rejects_raw_api_key() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_path = write_config(
            dir,
            r#"
[providers.openai]
api_key = "sk-raw-literal"
"#,
        )?;

        let err = match load_from(&config_path) {
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
        let config_path = write_config(
            dir,
            r#"
[providers.openai]
api_key = "store:openai"
"#,
        )?;

        let config = load_from(&config_path).map_err(|e| format!("load should succeed: {e:?}"))?;
        let openai = config.providers.get("openai").ok_or("openai provider")?;
        assert_eq!(openai.api_key.as_deref(), Some("store:openai"));
        Ok(())
    })
}

#[test]
fn load_accepts_env_reference() -> Result<()> {
    with_xdg_config_home(|dir| {
        let config_path = write_config(
            dir,
            r#"
[providers.openai]
api_key = "env:MY_CUSTOM_KEY"
"#,
        )?;

        let config = load_from(&config_path).map_err(|e| format!("load should succeed: {e:?}"))?;
        let openai = config.providers.get("openai").ok_or("openai provider")?;
        assert_eq!(openai.api_key.as_deref(), Some("env:MY_CUSTOM_KEY"));
        Ok(())
    })
}
