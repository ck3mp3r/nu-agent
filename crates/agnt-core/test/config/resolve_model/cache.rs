use super::*;

#[test]
fn test_resolve_model_cache_output_token_handling() -> Result<()> {
    // -- Setup & Fixtures
    // (name, explicit max_output_tokens, keep cache, expected result)
    let cases: Vec<(&str, Option<u32>, bool, Option<u32>)> = vec![
        ("unset is filled from cache", None, true, Some(4096)),
        ("explicit lower is preserved", Some(1024), true, Some(1024)),
        (
            "explicit higher is clamped to cache",
            Some(65536),
            true,
            Some(4096),
        ),
        (
            "no cache leaves it unchanged",
            Some(2048),
            false,
            Some(2048),
        ),
    ];

    // -- Exec & Check
    for (name, explicit, keep_cache, expected) in cases {
        let mut plugin_config = plugin_config_with_cache();
        if !keep_cache {
            plugin_config.models_cache = None;
        }
        let role_config = ModelRoleConfig {
            model: "openai/gpt-4".to_string(),
            max_output_tokens: explicit,
            ..ModelRoleConfig::default()
        };
        let config = plugin_config
            .resolve_model(&role_config)
            .map_err(|e| format!("{name}: {e:?}"))?;
        assert_eq!(config.max_output_tokens, expected, "{name}");
    }
    Ok(())
}

#[test]
fn test_resolve_model_cache_limits_fill_through_the_declared_catalog() -> Result<()> {
    // -- Setup & Fixtures: the config names the provider `copilot` and declares
    // the cache section it serves; the cache files it under `github-copilot`.
    let mut providers = HashMap::new();
    providers.insert(
        "copilot".to_string(),
        ProviderConfig {
            catalog: Some("github-copilot".to_string()),
            ..ProviderConfig::default()
        },
    );
    let mut cache_providers = HashMap::new();
    cache_providers.insert(
        "github-copilot".to_string(),
        crate::config::models_cache::ProviderSpec {
            id: "github-copilot".to_string(),
            name: "GitHub Copilot".to_string(),
            env: vec![],
            api: None,
            models: HashMap::from([(
                "claude-sonnet-4.6".to_string(),
                crate::config::models_cache::ModelSpec {
                    id: "claude-sonnet-4.6".to_string(),
                    name: "Claude Sonnet 4.6".to_string(),
                    tool_call: true,
                    limit: crate::config::models_cache::ModelLimit {
                        context: 200_000,
                        output: 64_000,
                    },
                    cost: None,
                    modalities: None,
                },
            )]),
        },
    );
    let plugin_config = PluginConfig {
        providers,
        models_cache: Some(crate::config::ModelsCache {
            providers: cache_providers,
        }),
        ..PluginConfig::default()
    };
    let role_config = ModelRoleConfig {
        model: "copilot/claude-sonnet-4.6".to_string(),
        ..ModelRoleConfig::default()
    };

    // -- Exec
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    // -- Check
    assert_eq!(
        config.max_context_tokens,
        Some(200_000),
        "the alias must reach the github-copilot section"
    );
    assert_eq!(config.max_output_tokens, Some(64_000));
    assert_eq!(config.model_context_tokens, Some(200_000));
    Ok(())
}

#[test]
fn test_resolve_model_carries_the_per_model_pricing() -> Result<()> {
    // -- Setup & Fixtures
    let mut providers = HashMap::new();
    providers.insert(
        "ollama".to_string(),
        ProviderConfig {
            models: HashMap::from([(
                "llama3.3".to_string(),
                ModelConfig {
                    pricing: Some(crate::config::models_cache::ModelCost {
                        input: 0.5,
                        output: 1.5,
                        cache_read: None,
                        cache_write: None,
                    }),
                    ..ModelConfig::default()
                },
            )]),
            ..ProviderConfig::default()
        },
    );
    let plugin_config = PluginConfig {
        providers,
        ..PluginConfig::default()
    };
    let role_config = ModelRoleConfig {
        model: "ollama/llama3.3".to_string(),
        ..ModelRoleConfig::default()
    };

    // -- Exec
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    // -- Check
    let pricing = config
        .pricing
        .ok_or("the resolved config must carry pricing")?;
    assert_eq!(pricing.input, 0.5);
    assert_eq!(pricing.output, 1.5);
    Ok(())
}

#[test]
fn test_resolve_model_leaves_pricing_unset_without_a_config_price() -> Result<()> {
    // -- Setup & Fixtures
    let plugin_config = plugin_config_with_cache();
    let role_config = ModelRoleConfig {
        model: "openai/gpt-4".to_string(),
        ..ModelRoleConfig::default()
    };

    // -- Exec
    let config = plugin_config
        .resolve_model(&role_config)
        .map_err(|e| format!("should resolve: {e:?}"))?;

    // -- Check
    assert_eq!(config.pricing, None);
    Ok(())
}
