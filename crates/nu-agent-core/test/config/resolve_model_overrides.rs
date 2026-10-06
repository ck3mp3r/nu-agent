//! `PluginConfig::resolve_model()` role-override, output-budget, and
//! repetition-guard tests.

use std::collections::HashMap;

use crate::config::{
    AgentsConfig, ModelConfig, ModelLimits, ModelRoleConfig, PluginConfig, ProviderConfig,
};
use crate::test_support::env_map;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

#[test]
fn test_plugin_config_resolve_model_role_level_overrides() -> Result<()> {
    let make_config = |model_temperature: Option<f64>| -> PluginConfig {
        let mut models = HashMap::new();
        models.insert(
            "gpt-4".to_string(),
            ModelConfig {
                name: None,
                temperature: model_temperature,
                preamble: None,
                tool_call: None,
                limit: None,
            },
        );
        let mut providers = HashMap::new();
        providers.insert(
            "openai".to_string(),
            ProviderConfig {
                name: None,
                api_key: None,
                base_url: None,
                provider: None,
                preamble: None,
                models,
            },
        );
        PluginConfig {
            models: {
                let mut m = HashMap::new();
                m.insert(
                    "default".to_string(),
                    ModelRoleConfig {
                        model: "openai/gpt-4".to_string(),
                        temperature: Some(0.5),
                        max_tokens: Some(2048),
                        max_context_tokens: Some(32000),
                        max_output_tokens: Some(1024),
                        max_tool_turns: Some(5),
                        max_tool_result_bytes: Some(10000),
                        model_context_tokens: Some(128000),
                        context_warning_threshold: Some(0.8f32),
                        max_retries: Some(5),
                        retry_base_delay_ms: Some(2000),
                        ..ModelRoleConfig::default()
                    },
                );
                m
            },
            providers,
            compaction: None,
            agents: AgentsConfig::default(),
            a2a_enabled: None,
            session_store: None,
            vault: None,
            models_cache: None,
            permissions: None,
            mcp: None,
            theme: None,
        }
    };

    let default_role = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        temperature: Some(0.5),
        max_tokens: Some(2048),
        max_context_tokens: Some(32000),
        max_output_tokens: Some(1024),
        max_tool_turns: Some(5),
        max_tool_result_bytes: Some(10000),
        model_context_tokens: Some(128000),
        context_warning_threshold: Some(0.8f32),
        max_retries: Some(5),
        retry_base_delay_ms: Some(2000),
        ..ModelRoleConfig::default()
    };

    // Case 1: no model-level temperature — role-level 0.5 must survive
    let cfg = make_config(None)
        .resolve_model(&default_role)
        .map_err(|e| format!("resolve: {e:?}"))?;
    assert_eq!(cfg.temperature, Some(0.5));
    assert_eq!(cfg.max_tokens, Some(2048));
    assert_eq!(cfg.max_context_tokens, Some(32000));
    assert_eq!(cfg.max_output_tokens, Some(1024));
    assert_eq!(cfg.max_tool_turns, Some(5));
    assert_eq!(cfg.max_tool_result_bytes, Some(10000));
    assert_eq!(cfg.model_context_tokens, Some(128000));
    assert_eq!(cfg.context_warning_threshold, Some(0.8f32));
    assert_eq!(cfg.max_retries, Some(5));
    assert_eq!(cfg.retry_base_delay_ms, Some(2000));

    // Case 2: role-level temperature 0.5 must beat model-level 0.9
    // (role config is highest priority within resolve_model)
    let cfg = make_config(Some(0.9))
        .resolve_model(&default_role)
        .map_err(|e| format!("resolve: {e:?}"))?;
    assert_eq!(cfg.temperature, Some(0.5));
    Ok(())
}

#[test]
fn test_resolve_model_output_budget_remedy_fields() -> Result<()> {
    // Role config sets both remedy fields; resolve_model must copy them into Config.
    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    output_budget_empty_remedy: Some("custom remedy".to_string()),
                    output_budget_remedy_mode: Some("shorter_response".to_string()),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: HashMap::new(),
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };
    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        output_budget_empty_remedy: Some("custom remedy".to_string()),
        output_budget_remedy_mode: Some("shorter_response".to_string()),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;
    assert_eq!(
        config.output_budget_empty_remedy,
        Some("custom remedy".to_string())
    );
    assert_eq!(
        config.output_budget_remedy_mode,
        Some("shorter_response".to_string())
    );
    Ok(())
}

#[test]
fn test_resolve_model_output_budget_raise_fields() -> Result<()> {
    // Role config sets the three raise fields; resolve_model must copy them into Config.
    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    output_budget_raise_enabled: Some(true),
                    output_budget_raise_multiplier: Some(3.0),
                    output_budget_raise_cap: Some(65536),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: HashMap::new(),
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };
    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        output_budget_raise_enabled: Some(true),
        output_budget_raise_multiplier: Some(3.0),
        output_budget_raise_cap: Some(65536),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;
    assert_eq!(config.output_budget_raise_enabled, Some(true));
    assert_eq!(config.output_budget_raise_multiplier, Some(3.0));
    assert_eq!(config.output_budget_raise_cap, Some(65536));
    Ok(())
}

#[test]
fn test_resolve_model_repetition_guard_none_when_unset() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: HashMap::new(),
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };
    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        ..ModelRoleConfig::default()
    };

    // -- Exec
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    // -- Check
    assert_eq!(config.repetition_guard, None);
    Ok(())
}

#[test]
fn test_resolve_model_repetition_guard_role_false() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: HashMap::new(),
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };
    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        repetition_guard: Some(false),
        ..ModelRoleConfig::default()
    };

    // -- Exec
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    // -- Check
    assert_eq!(config.repetition_guard, Some(false));
    Ok(())
}

#[test]
fn test_resolve_model_repetition_guard_role_overrides_env() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: HashMap::new(),
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };
    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        repetition_guard: Some(true),
        ..ModelRoleConfig::default()
    };

    // -- Exec
    let env = env_map(&[("AGENT_REPETITION_GUARD", "false")]);
    let resolved = plugin_config
        .resolve_model_with(&role_config, &env)
        .map_err(|e| format!("should resolve: {e:?}"))
        .ok();

    // -- Check
    let config = resolved.ok_or("should resolve")?;
    assert_eq!(config.repetition_guard, Some(true));
    Ok(())
}

#[test]
fn test_resolve_model_role_max_output_tokens_overrides_model_limit() -> Result<()> {
    // Role-level max_output_tokens must beat the model-level limit from the
    // provider's models map (role config is highest priority within resolve_model).
    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "openai/gpt-4".to_string(),
                    max_output_tokens: Some(1024),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "openai".to_string(),
                ProviderConfig {
                    name: None,
                    api_key: None,
                    base_url: None,
                    provider: None,
                    preamble: None,
                    models: {
                        let mut models = HashMap::new();
                        models.insert(
                            "gpt-4".to_string(),
                            ModelConfig {
                                name: None,
                                temperature: None,
                                preamble: None,
                                tool_call: None,
                                limit: Some(ModelLimits {
                                    context: Some(128000),
                                    output: Some(8192),
                                }),
                            },
                        );
                        models
                    },
                },
            );
            providers
        },
        compaction: None,
        agents: AgentsConfig::default(),
        a2a_enabled: None,
        session_store: None,
        vault: None,
        models_cache: None,
        permissions: None,
        mcp: None,
        theme: None,
    };

    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        max_output_tokens: Some(1024),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    // Role-level 1024 must win over model-level 8192
    assert_eq!(config.max_output_tokens, Some(1024));
    // Context limit is not overridden by role, so model-level survives
    assert_eq!(config.max_context_tokens, Some(128000));
    Ok(())
}
