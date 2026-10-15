//! Persona model preamble re-resolution tests for `runtime_build`.

use super::*;

// ── apply_persona_model preamble re-resolution tests ────────────────────────

#[test]
fn apply_persona_model_re_resolves_preamble_for_literal_model() -> Result<()> {
    let mut models = HashMap::new();
    models.insert(
        "claude-sonnet-4-6".to_string(),
        make_model_config(Some("You are Claude Sonnet.")),
    );
    let mut providers = HashMap::new();
    providers.insert(
        "anthropic".to_string(),
        make_provider_config(Some("You are Anthropic."), models),
    );
    let mut plugin_models = HashMap::new();
    plugin_models.insert(
        "default".to_string(),
        ModelRoleConfig {
            model: "openai/gpt-4o".to_string(),
            ..ModelRoleConfig::default()
        },
    );
    let pc = make_plugin_config_with_providers(plugin_models, providers);

    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        preamble: Some("You are OpenAI GPT-4o.".to_string()),
        ..Config::default()
    };

    let result = super::apply_persona_model(
        &mut config,
        Some(&pc),
        Some("anthropic/claude-sonnet-4-6"),
        false,
    );

    let applied = result.map_err(|e| format!("{e:?}"))?;
    assert!(applied);
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-sonnet-4-6");
    // Preamble should be re-resolved for the new model
    assert_eq!(config.preamble, Some("You are Claude Sonnet.".to_string()));
    Ok(())
}

#[test]
fn apply_persona_model_re_resolves_preamble_for_role_label() -> Result<()> {
    let mut models = HashMap::new();
    models.insert(
        "claude-opus-4".to_string(),
        make_model_config(Some("You are Claude Opus.")),
    );
    let mut providers = HashMap::new();
    providers.insert(
        "anthropic".to_string(),
        make_provider_config(Some("You are Anthropic."), models),
    );
    let mut plugin_models = HashMap::new();
    plugin_models.insert(
        "default".to_string(),
        ModelRoleConfig {
            model: "openai/gpt-4o".to_string(),
            ..ModelRoleConfig::default()
        },
    );
    plugin_models.insert(
        "heavy".to_string(),
        ModelRoleConfig {
            model: "anthropic/claude-opus-4".to_string(),
            ..ModelRoleConfig::default()
        },
    );
    let pc = make_plugin_config_with_providers(plugin_models, providers);

    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        preamble: Some("You are OpenAI GPT-4o.".to_string()),
        ..Config::default()
    };

    let result = super::apply_persona_model(&mut config, Some(&pc), Some("heavy"), false);

    let applied = result.map_err(|e| format!("{e:?}"))?;
    assert!(applied);
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-opus-4");
    // Preamble should be re-resolved for the heavy model
    assert_eq!(config.preamble, Some("You are Claude Opus.".to_string()));
    Ok(())
}

#[test]
fn apply_persona_model_skipped_does_not_change_preamble() -> Result<()> {
    let mut models = HashMap::new();
    models.insert(
        "claude-sonnet-4-6".to_string(),
        make_model_config(Some("You are Claude Sonnet.")),
    );
    let mut providers = HashMap::new();
    providers.insert(
        "anthropic".to_string(),
        make_provider_config(Some("You are Anthropic."), models),
    );
    let mut plugin_models = HashMap::new();
    plugin_models.insert(
        "default".to_string(),
        ModelRoleConfig {
            model: "openai/gpt-4o".to_string(),
            ..ModelRoleConfig::default()
        },
    );
    let pc = make_plugin_config_with_providers(plugin_models, providers);

    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        preamble: Some("Original preamble.".to_string()),
        ..Config::default()
    };

    // CLI model provided — persona model should be skipped
    let result = super::apply_persona_model(
        &mut config,
        Some(&pc),
        Some("anthropic/claude-sonnet-4-6"),
        true, // cli_model_provided
    );

    let applied = result.map_err(|e| format!("{e:?}"))?;
    assert!(!applied);
    // Preamble should NOT change
    assert_eq!(config.preamble, Some("Original preamble.".to_string()));
    Ok(())
}

#[test]
fn apply_persona_model_none_does_not_change_preamble() -> Result<()> {
    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        preamble: Some("Original preamble.".to_string()),
        ..Config::default()
    };

    let result = super::apply_persona_model(&mut config, None, None, false);

    let applied = result.map_err(|e| format!("{e:?}"))?;
    assert!(!applied);
    // Preamble should NOT change
    assert_eq!(config.preamble, Some("Original preamble.".to_string()));
    Ok(())
}
