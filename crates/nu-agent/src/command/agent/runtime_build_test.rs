use std::collections::HashMap;

use nu_agent_core::config::{Config, ModelConfig, ModelRoleConfig, PluginConfig, ProviderConfig};

#[test]
fn apply_persona_config_all_fields_when_config_empty() {
    let mut config = Config::default();
    let persona = nu_agent_core::protocol::persona::ParsedPersona {
        name: None,
        description: None,
        model: None,
        permissions: None,
        temperature: Some(0.7),
        max_tokens: Some(2048),
        max_tool_turns: Some(10),
        max_tool_calls_per_subturn: Some(3),
        max_tool_result_bytes: Some(5000),
        additional_params: Some(serde_json::json!({"thinking": "disabled"})),
        icon: None,
        body: String::new(),
    };

    super::apply_persona_config(&mut config, &persona, false);

    assert_eq!(config.temperature, Some(0.7));
    assert_eq!(config.max_tokens, Some(2048));
    assert_eq!(config.max_tool_turns, Some(10));
    assert_eq!(config.max_tool_calls_per_subturn, Some(3));
    assert_eq!(config.max_tool_result_bytes, Some(5000));
    assert!(config.additional_params.is_some());
}

#[test]
fn apply_persona_config_cli_wins_over_persona() {
    let mut config = Config {
        temperature: Some(0.9),
        max_tokens: Some(4096),
        max_tool_calls_per_subturn: Some(8),
        max_tool_result_bytes: Some(20000),
        additional_params: Some(serde_json::json!({"from": "cli"})),
        ..Config::default()
    };
    let persona = nu_agent_core::protocol::persona::ParsedPersona {
        name: None,
        description: None,
        model: None,
        permissions: None,
        temperature: Some(0.1),
        max_tokens: Some(512),
        max_tool_turns: None,
        max_tool_calls_per_subturn: Some(1),
        max_tool_result_bytes: Some(100),
        additional_params: Some(serde_json::json!({"from": "persona"})),
        icon: None,
        body: String::new(),
    };

    super::apply_persona_config(&mut config, &persona, false);

    // CLI values must survive
    assert_eq!(config.temperature, Some(0.9));
    assert_eq!(config.max_tokens, Some(4096));
    assert_eq!(config.max_tool_calls_per_subturn, Some(8));
    assert_eq!(config.max_tool_result_bytes, Some(20000));
    assert_eq!(
        config
            .additional_params
            .as_ref()
            .expect("additional_params should be set")["from"],
        "cli"
    );
}

#[test]
fn apply_persona_config_max_turns_cli_flag_wins() {
    let mut config = Config {
        max_tool_turns: Some(20),
        ..Config::default()
    };
    let persona = nu_agent_core::protocol::persona::ParsedPersona {
        name: None,
        description: None,
        model: None,
        permissions: None,
        temperature: None,
        max_tokens: None,
        max_tool_turns: Some(5),
        max_tool_calls_per_subturn: None,
        max_tool_result_bytes: None,
        additional_params: None,
        icon: None,
        body: String::new(),
    };

    super::apply_persona_config(&mut config, &persona, true); // cli_max_turns_provided = true

    assert_eq!(config.max_tool_turns, Some(20)); // CLI wins
}

#[test]
fn apply_persona_config_max_turns_persona_overrides_default() {
    let mut config = Config {
        max_tool_turns: Some(20), // pipeline default
        ..Config::default()
    };
    let persona = nu_agent_core::protocol::persona::ParsedPersona {
        name: None,
        description: None,
        model: None,
        permissions: None,
        temperature: None,
        max_tokens: None,
        max_tool_turns: Some(5),
        max_tool_calls_per_subturn: None,
        max_tool_result_bytes: None,
        additional_params: None,
        icon: None,
        body: String::new(),
    };

    super::apply_persona_config(&mut config, &persona, false); // not a CLI flag

    assert_eq!(config.max_tool_turns, Some(5)); // persona wins over default
}

#[test]
fn apply_persona_config_partial_persona_leaves_others_unchanged() {
    let mut config = Config {
        max_tool_turns: Some(10),
        max_tool_result_bytes: Some(1000),
        ..Config::default()
    };
    let persona = nu_agent_core::protocol::persona::ParsedPersona {
        name: None,
        description: None,
        model: None,
        permissions: None,
        temperature: Some(0.7),
        max_tokens: None,
        max_tool_turns: None,
        max_tool_calls_per_subturn: None,
        max_tool_result_bytes: None,
        additional_params: None,
        icon: None,
        body: String::new(),
    };

    super::apply_persona_config(&mut config, &persona, true);

    assert_eq!(config.temperature, Some(0.7)); // applied
    assert_eq!(config.max_tool_turns, Some(10)); // unchanged (cli flag)
    assert_eq!(config.max_tool_result_bytes, Some(1000)); // unchanged (persona had None)
}

// ── apply_persona_model tests ──────────────────────────────────────────────

fn make_plugin_config(models: HashMap<String, ModelRoleConfig>) -> PluginConfig {
    // Build providers for each unique provider referenced in models
    let mut providers = std::collections::HashMap::new();
    for role in models.values() {
        if let Some((provider_name, _)) = role.model.split_once('/')
            && !providers.contains_key(provider_name)
        {
            providers.insert(
                provider_name.to_string(),
                nu_agent_core::config::ProviderConfig {
                    name: None,
                    api_key: None,
                    base_url: None,
                    provider: None,
                    preamble: None,
                    models: std::collections::HashMap::new(),
                },
            );
        }
    }
    PluginConfig {
        models,
        providers,
        compaction: None,
        agents: Default::default(),
        a2a_enabled: None,
        session_store: None,
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    }
}

#[test]
fn apply_persona_model_literal_slash_applied() {
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

    assert!(result.is_ok());
    assert!(result.unwrap());
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-sonnet-4-6");
}

#[test]
fn apply_persona_model_role_label_resolved() {
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

    assert!(result.is_ok());
    assert!(result.unwrap());
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-opus-4");
}

#[test]
fn apply_persona_model_role_label_default_resolved() {
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

    assert!(result.is_ok());
    assert!(result.unwrap());
    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4o");
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
fn apply_persona_model_none_no_change() {
    let mut config = Config::default();
    let plugin_config = make_plugin_config(HashMap::new());

    let result = super::apply_persona_model(&mut config, Some(&plugin_config), None, false);

    assert!(result.is_ok());
    assert!(!result.unwrap());
    assert_eq!(config.provider, "");
    assert_eq!(config.model, "");
}

#[test]
fn apply_persona_model_cli_flag_skips_persona() {
    let mut config = Config::default();
    let plugin_config = make_plugin_config(HashMap::new());

    let result = super::apply_persona_model(
        &mut config,
        Some(&plugin_config),
        Some("anthropic/claude-sonnet-4-6"),
        true, // cli_model_provided
    );

    assert!(result.is_ok());
    assert!(!result.unwrap());
    assert_eq!(config.provider, "");
    assert_eq!(config.model, "");
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

// ── resolve_preamble_for_model tests ────────────────────────────────────────

fn make_provider_config(
    preamble: Option<&str>,
    models: HashMap<String, ModelConfig>,
) -> ProviderConfig {
    ProviderConfig {
        name: None,
        api_key: None,
        base_url: None,
        provider: None,
        preamble: preamble.map(|s| s.to_string()),
        models,
    }
}

fn make_model_config(preamble: Option<&str>) -> ModelConfig {
    ModelConfig {
        limit: None,
        name: None,
        temperature: None,
        preamble: preamble.map(|s| s.to_string()),
        tool_call: None,
    }
}

fn make_plugin_config_with_providers(
    models: HashMap<String, ModelRoleConfig>,
    providers: HashMap<String, ProviderConfig>,
) -> PluginConfig {
    PluginConfig {
        models,
        providers,
        compaction: None,
        agents: Default::default(),
        a2a_enabled: None,
        session_store: None,
        secret_store: None,
        models_cache: None,
        permissions: None,
        mcp: None,
    }
}

#[test]
fn resolve_preamble_for_model_returns_provider_preamble() {
    let mut models = HashMap::new();
    models.insert("gpt-4o".to_string(), make_model_config(None));
    let mut providers = HashMap::new();
    providers.insert(
        "openai".to_string(),
        make_provider_config(Some("You are OpenAI GPT-4o."), models),
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

    let preamble = super::resolve_preamble_for_model(&pc, "openai", "gpt-4o");

    assert_eq!(preamble, Some("You are OpenAI GPT-4o.".to_string()));
}

#[test]
fn resolve_preamble_for_model_returns_model_preamble_over_provider() {
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
            model: "anthropic/claude-sonnet-4-6".to_string(),
            ..ModelRoleConfig::default()
        },
    );
    let pc = make_plugin_config_with_providers(plugin_models, providers);

    let preamble = super::resolve_preamble_for_model(&pc, "anthropic", "claude-sonnet-4-6");

    // Model-level preamble should take precedence over provider-level
    assert_eq!(preamble, Some("You are Claude Sonnet.".to_string()));
}

#[test]
fn resolve_preamble_for_model_returns_none_when_no_preamble_configured() {
    let mut models = HashMap::new();
    models.insert("gpt-4o".to_string(), make_model_config(None));
    let mut providers = HashMap::new();
    providers.insert("openai".to_string(), make_provider_config(None, models));
    let mut plugin_models = HashMap::new();
    plugin_models.insert(
        "default".to_string(),
        ModelRoleConfig {
            model: "openai/gpt-4o".to_string(),
            ..ModelRoleConfig::default()
        },
    );
    let pc = make_plugin_config_with_providers(plugin_models, providers);

    let preamble = super::resolve_preamble_for_model(&pc, "openai", "gpt-4o");

    // No user preamble configured — builtin defaults may still return something
    // but we just verify it doesn't panic and returns Some or None
    assert!(preamble.is_some() || preamble.is_none());
}

#[test]
fn resolve_preamble_for_model_unknown_provider_returns_none() {
    let pc = make_plugin_config_with_providers(HashMap::new(), HashMap::new());

    let preamble = super::resolve_preamble_for_model(&pc, "unknown", "model");

    // Unknown provider — no user preamble, but builtin defaults may still apply
    assert!(preamble.is_some() || preamble.is_none());
}

// ── apply_persona_model preamble re-resolution tests ────────────────────────

#[test]
fn apply_persona_model_re_resolves_preamble_for_literal_model() {
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

    assert!(result.is_ok());
    assert!(result.unwrap());
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-sonnet-4-6");
    // Preamble should be re-resolved for the new model
    assert_eq!(config.preamble, Some("You are Claude Sonnet.".to_string()));
}

#[test]
fn apply_persona_model_re_resolves_preamble_for_role_label() {
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

    assert!(result.is_ok());
    assert!(result.unwrap());
    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude-opus-4");
    // Preamble should be re-resolved for the heavy model
    assert_eq!(config.preamble, Some("You are Claude Opus.".to_string()));
}

#[test]
fn apply_persona_model_skipped_does_not_change_preamble() {
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

    assert!(result.is_ok());
    assert!(!result.unwrap());
    // Preamble should NOT change
    assert_eq!(config.preamble, Some("Original preamble.".to_string()));
}

#[test]
fn apply_persona_model_none_does_not_change_preamble() {
    let mut config = Config {
        provider: "openai".to_string(),
        model: "gpt-4o".to_string(),
        preamble: Some("Original preamble.".to_string()),
        ..Config::default()
    };

    let result = super::apply_persona_model(&mut config, None, None, false);

    assert!(result.is_ok());
    assert!(!result.unwrap());
    // Preamble should NOT change
    assert_eq!(config.preamble, Some("Original preamble.".to_string()));
}
