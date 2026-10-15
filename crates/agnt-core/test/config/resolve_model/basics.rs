use super::*;

#[test]
fn test_model_role_config_default() {
    let cfg = ModelRoleConfig::default();
    assert_eq!(cfg.model, "");
    assert_eq!(cfg.temperature, None);
    assert_eq!(cfg.max_tokens, None);
    assert_eq!(cfg.max_context_tokens, None);
    assert_eq!(cfg.max_output_tokens, None);
    assert_eq!(cfg.max_tool_turns, None);
    assert_eq!(cfg.max_tool_result_bytes, None);
    assert_eq!(cfg.max_tool_calls_per_subturn, None);
    assert_eq!(cfg.model_context_tokens, None);
    assert_eq!(cfg.context_warning_threshold, None);
    assert_eq!(cfg.additional_params, None);
    assert_eq!(cfg.read_timeout_secs, None);
    assert_eq!(cfg.max_retries, None);
    assert_eq!(cfg.retry_base_delay_ms, None);
}

#[test]
fn test_resolve_model_basic() -> Result<()> {
    // Test resolving a basic model specification
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
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "openai".to_string(),
                ProviderConfig {
                    name: None,
                    api_key: Some("sk-test123".to_string()),
                    base_url: None,
                    provider: None,
                    preamble: None,
                    models: {
                        let mut models = HashMap::new();
                        models.insert(
                            "gpt-4".to_string(),
                            ModelConfig {
                                name: None,
                                temperature: Some(0.7),
                                preamble: None,
                                tool_call: Some(true),
                                limit: Some(ModelLimits {
                                    context: Some(128000),
                                    output: Some(4096),
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
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-4");
    assert_eq!(config.api_key, Some("sk-test123".to_string()));
    assert_eq!(config.temperature, Some(0.7));
    assert_eq!(config.max_context_tokens, Some(128000));
    assert_eq!(config.max_output_tokens, Some(4096));
    Ok(())
}
