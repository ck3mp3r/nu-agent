use super::*;

#[test]
fn test_resolve_model_merges_provider_and_model_fields() -> Result<()> {
    // -- Setup & Fixtures
    // Case A: a custom provider field (github-copilot style) selects the impl.
    let copilot = PluginConfig {
        models: HashMap::new(),
        providers: {
            let mut providers = HashMap::new();
            providers.insert(
                "copilot".to_string(),
                ProviderConfig {
                    name: Some("GitHub Copilot".to_string()),
                    api_key: Some("ghcp-token".to_string()),
                    base_url: Some("https://api.githubcopilot.com".to_string()),
                    provider: Some("openai".to_string()),
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
    };

    // Case B: model limits from the provider's models map are merged in.
    let limits = PluginConfig {
        models: HashMap::new(),
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
                    catalog: None,
                    models: {
                        let mut models = HashMap::new();
                        models.insert(
                            "gpt-4".to_string(),
                            ModelConfig {
                                name: Some("GPT-4".to_string()),
                                temperature: None,
                                preamble: None,
                                tool_call: None,
                                limit: Some(ModelLimits {
                                    context: Some(128000),
                                    output: Some(8192),
                                }),
                                pricing: None,
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

    // -- Exec & Check
    let config = copilot
        .resolve_model(&ModelRoleConfig {
            model: "copilot/claude".to_string(),
            ..ModelRoleConfig::default()
        })
        .map_err(|e| format!("should resolve: {e:?}"))?;
    assert_eq!(config.provider, "copilot");
    assert_eq!(config.model, "claude");
    assert_eq!(config.api_key, Some("ghcp-token".to_string()));
    assert_eq!(
        config.base_url,
        Some("https://api.githubcopilot.com".to_string())
    );
    assert_eq!(config.provider_impl, Some("openai".to_string()));

    let config = limits
        .resolve_model(&ModelRoleConfig {
            model: "openai/gpt-4".to_string(),
            ..ModelRoleConfig::default()
        })
        .map_err(|e| format!("should resolve: {e:?}"))?;
    assert_eq!(config.max_context_tokens, Some(128000));
    assert_eq!(config.max_output_tokens, Some(8192));
    Ok(())
}
