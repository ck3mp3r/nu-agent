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
