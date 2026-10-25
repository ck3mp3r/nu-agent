use super::*;

#[test]
fn test_resolve_model_with_env_fallback() -> Result<()> {
    // Test that resolve_model falls back to env vars when provider doesn't have api_key
    let plugin_config = PluginConfig {
        models: {
            let mut m = HashMap::new();
            m.insert(
                "default".to_string(),
                ModelRoleConfig {
                    model: "anthropic/claude".to_string(),
                    ..ModelRoleConfig::default()
                },
            );
            m
        },
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "anthropic".to_string(),
                ProviderConfig {
                    name: None,
                    api_key: None, // No API key in config
                    base_url: None,
                    provider: None,
                    preamble: None,
                    catalog: None,
                    models: HashMap::new(),
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
        hooks: Vec::new(),
    };

    let role_config = ModelRoleConfig {
        model: "anthropic/claude".to_string(),
        ..ModelRoleConfig::default()
    };
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    assert_eq!(config.provider, "anthropic");
    assert_eq!(config.model, "claude");
    // API key should be None (will be read from env later)
    assert_eq!(config.api_key, None);
    Ok(())
}
