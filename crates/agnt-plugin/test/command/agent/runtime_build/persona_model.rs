//! Persona model resolution tests for `runtime_build`.

use super::*;

// ── apply_persona_model tests ──────────────────────────────────────────────

#[test]
fn apply_persona_model_literal_slash_applied() -> Result<()> {
    let mut config = Config::default();
    let mut models = HashMap::new();
    models.insert(
        "default".to_string(),
        ModelRoleConfig {
            model: "anthropic/claude-sonnet-4-6".to_string(),
            ..ModelRoleConfig::default()
        },
    );
    let plugin_config = make_plugin_config(models);

    let result = super::apply_persona_model(
        &mut config,
        Some(&plugin_config),
        Some("anthropic/claude-sonnet-4-6"),
        false,
    );

    let applied = result.map_err(|e| format!("{e:?}"))?;
    assert!(applied);
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-sonnet-4-6");
    Ok(())
}

#[test]
fn apply_persona_model_role_label_resolved() -> Result<()> {
    let mut config = Config::default();
    let mut models = HashMap::new();
    models.insert(
        "heavy".to_string(),
        ModelRoleConfig {
            model: "anthropic/claude-opus-4".to_string(),
            ..ModelRoleConfig::default()
        },
    );
    let plugin_config = make_plugin_config(models);

    let result =
        super::apply_persona_model(&mut config, Some(&plugin_config), Some("heavy"), false);

    let applied = result.map_err(|e| format!("{e:?}"))?;
    assert!(applied);
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-opus-4");
    Ok(())
}

#[test]
fn apply_persona_model_role_label_default_resolved() -> Result<()> {
    let mut config = Config::default();
    let mut models = HashMap::new();
    models.insert(
        "default".to_string(),
        ModelRoleConfig {
            model: "openai/gpt-4o".to_string(),
            ..ModelRoleConfig::default()
        },
    );
    let plugin_config = make_plugin_config(models);

    let result =
        super::apply_persona_model(&mut config, Some(&plugin_config), Some("default"), false);

    let applied = result.map_err(|e| format!("{e:?}"))?;
    assert!(applied);
    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4o");
    Ok(())
}

#[test]
fn apply_persona_model_unknown_role_returns_error() {
    let mut config = Config::default();
    let mut models = HashMap::new();
    models.insert(
        "default".to_string(),
        ModelRoleConfig {
            model: "openai/gpt-4o".to_string(),
            ..ModelRoleConfig::default()
        },
    );
    models.insert(
        "heavy".to_string(),
        ModelRoleConfig {
            model: "anthropic/claude-opus-4".to_string(),
            ..ModelRoleConfig::default()
        },
    );
    let plugin_config = make_plugin_config(models);

    let result = super::apply_persona_model(&mut config, Some(&plugin_config), Some("foo"), false);

    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("foo"),
        "error should mention unknown role: {msg}"
    );
    assert!(
        msg.contains("default"),
        "error should list available roles: {msg}"
    );
    assert!(
        msg.contains("heavy"),
        "error should list available roles: {msg}"
    );
}

#[test]
fn apply_persona_model_role_not_configured_returns_error() {
    let mut config = Config::default();
    let mut models = HashMap::new();
    models.insert(
        "default".to_string(),
        ModelRoleConfig {
            model: "openai/gpt-4o".to_string(),
            ..ModelRoleConfig::default()
        },
    );
    // "heavy" is not in models
    let plugin_config = make_plugin_config(models);

    let result =
        super::apply_persona_model(&mut config, Some(&plugin_config), Some("heavy"), false);

    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("heavy"), "error should mention role: {msg}");
}

#[test]
fn apply_persona_model_none_no_change() -> Result<()> {
    let mut config = Config::default();
    let plugin_config = make_plugin_config(HashMap::new());

    let result = super::apply_persona_model(&mut config, Some(&plugin_config), None, false);

    let applied = result.map_err(|e| format!("{e:?}"))?;
    assert!(!applied);
    assert_eq!(config.provider, "");
    assert_eq!(config.model, "");
    Ok(())
}

#[test]
fn apply_persona_model_cli_flag_skips_persona() -> Result<()> {
    let mut config = Config::default();
    let plugin_config = make_plugin_config(HashMap::new());

    let result = super::apply_persona_model(
        &mut config,
        Some(&plugin_config),
        Some("anthropic/claude-sonnet-4-6"),
        true, // cli_model_provided
    );

    let applied = result.map_err(|e| format!("{e:?}"))?;
    assert!(!applied);
    assert_eq!(config.provider, "");
    assert_eq!(config.model, "");
    Ok(())
}

#[test]
fn apply_persona_model_no_plugin_config_role_label_returns_error() {
    let mut config = Config::default();

    let result = super::apply_persona_model(&mut config, None, Some("heavy"), false);

    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("heavy"), "error should mention role: {msg}");
    assert!(
        msg.contains("plugin config"),
        "error should mention missing plugin config: {msg}"
    );
}
