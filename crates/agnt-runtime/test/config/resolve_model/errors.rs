use super::*;

#[test]
fn test_resolve_model_invalid_format() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = PluginConfig {
        models: HashMap::new(),
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
        hooks: Vec::new(),
        paths: None,
    };

    // -- Exec & Check
    // No slash separator
    let result = plugin_config.resolve_model(&ModelRoleConfig {
        model: "openaigpt4".to_string(),
        ..ModelRoleConfig::default()
    });
    let err = match result {
        Ok(_) => return Err("missing slash should fail resolution".into()),
        Err(e) => e,
    };
    assert!(err.contains("Expected 'provider/model'"));

    // Empty provider and empty model parts
    for model in ["/gpt-4", "openai/"] {
        let result = plugin_config.resolve_model(&ModelRoleConfig {
            model: model.to_string(),
            ..ModelRoleConfig::default()
        });
        assert!(result.is_err(), "{model} should fail resolution");
    }
    Ok(())
}

#[test]
fn test_resolve_model_resolves_without_provider_or_model_config() -> Result<()> {
    // -- Setup & Fixtures
    // An empty env map isolates the test from AGENT_* vars in the surrounding
    // environment (e.g. nix build sets AGENT_TEMPERATURE), which would
    // otherwise be picked up by Config::from_env and break the default
    // expectations.
    let env = EnvMap::new();
    let no_providers = PluginConfig {
        models: HashMap::new(),
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
        hooks: Vec::new(),
        paths: None,
    };
    let with_provider = PluginConfig {
        models: HashMap::new(),
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
        paths: None,
    };

    // -- Exec & Check
    // Provider block is optional — an unknown provider resolves from env only.
    let config = no_providers
        .resolve_model(&ModelRoleConfig {
            model: "unknown/model".to_string(),
            ..ModelRoleConfig::default()
        })
        .map_err(|e| format!("unknown provider should resolve: {e:?}"))?;
    assert_eq!(config.provider, "unknown");
    assert_eq!(config.model, "model");

    // A model missing from the provider's models map still resolves with defaults.
    let config = with_provider
        .resolve_model_with(
            &ModelRoleConfig {
                model: "openai/gpt-3.5-turbo".to_string(),
                ..ModelRoleConfig::default()
            },
            &env,
        )
        .map_err(|e| format!("should resolve: {e:?}"))?;
    assert_eq!(config.provider, "openai");
    assert_eq!(config.model, "gpt-3.5-turbo");
    assert_eq!(config.api_key, Some("sk-test123".to_string()));
    assert_eq!(config.temperature, None);
    Ok(())
}
