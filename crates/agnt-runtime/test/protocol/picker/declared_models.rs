//! The picker lists the models each config block can run.

use super::*;

// region:    --- runnable models per block

#[test]
fn test_picker_build_model_picker_catalog_lists_runnable_models_per_block() -> Result<()> {
    // -- Setup & Fixtures: the cache files ollama's models under
    // `ollama-cloud`. The local `ollama` block declares no catalog, so it
    // serves only what it declares and joins no metadata; the `ollama-cloud`
    // block's name is its own section, so every row is runnable through it.
    let cache = models_cache(&[provider_spec_with_cost(
        "ollama-cloud",
        "Ollama Cloud",
        &[
            ("glm-5.1", "GLM 5.1", 200_000, 128_000, Some((1.4, 4.4))),
            ("llama3.3", "Llama 3.3", 128_000, 4_096, None),
            ("qwen3", "Qwen 3", 32_768, 8_192, None),
            ("mistral-small", "Mistral Small", 32_768, 8_192, None),
            (
                "deepseek-v4.1-flash",
                "DeepSeek V4.1 Flash",
                128_000,
                8_192,
                Some((0.2, 0.8)),
            ),
        ],
    )]);
    let config = plugin_config(&[
        ("ollama", &["llama3.3", "qwen3", "mistral-small"]),
        (
            "ollama-cloud",
            &["deepseek-v4.1-flash", "deepseek-v4.1-pro"],
        ),
    ]);

    // -- Exec
    let options = build_model_picker_catalog(Some(&cache), &config, "ollama/llama3.3");

    // -- Check
    let mut identities: Vec<&str> = options.iter().map(|o| o.identity.as_str()).collect();
    identities.sort_unstable();
    assert_eq!(
        identities,
        vec![
            "ollama-cloud/deepseek-v4.1-flash",
            "ollama-cloud/deepseek-v4.1-pro",
            "ollama-cloud/glm-5.1",
            "ollama-cloud/llama3.3",
            "ollama-cloud/mistral-small",
            "ollama-cloud/qwen3",
            "ollama/llama3.3",
            "ollama/mistral-small",
            "ollama/qwen3",
        ],
        "the aliased block lists declared models; the owning block lists its section plus declared extras"
    );

    let local = options
        .iter()
        .find(|o| o.identity == "ollama/llama3.3")
        .ok_or("catalog must hold the declared local model")?;
    assert_eq!(
        local.provider_display_name, "ollama",
        "the group header must be the config block name"
    );
    assert!(local.configured);
    assert_eq!(
        local.context_window, None,
        "a block with no declared catalog joins no metadata"
    );
    assert_eq!(local.max_output, None);
    assert_eq!(local.cost, None);

    let cloud = options
        .iter()
        .find(|o| o.identity == "ollama-cloud/deepseek-v4.1-flash")
        .ok_or("catalog must hold the declared cloud model")?;
    assert_eq!(cloud.provider_display_name, "ollama-cloud");
    assert!(cloud.configured);
    assert_eq!(cloud.context_window, Some(128_000));
    assert_eq!(cloud.cost.as_deref(), Some("$0.20/$0.80"));

    let undeclared = options
        .iter()
        .find(|o| o.identity == "ollama-cloud/glm-5.1")
        .ok_or("the owning block must list the section row it does not declare")?;
    assert!(
        !undeclared.configured,
        "a section row the block does not declare is not configured"
    );
    assert_eq!(undeclared.context_window, Some(200_000));
    assert_eq!(undeclared.cost.as_deref(), Some("$1.40/$4.40"));

    for identity in [
        "ollama-cloud/llama3.3",
        "ollama-cloud/mistral-small",
        "ollama-cloud/qwen3",
    ] {
        let row = options
            .iter()
            .find(|o| o.identity == identity)
            .ok_or("the owning block must list every section row")?;
        assert!(!row.configured, "{identity} is not declared by the block");
    }

    let uncached = options
        .iter()
        .find(|o| o.identity == "ollama-cloud/deepseek-v4.1-pro")
        .ok_or("catalog must hold the declared but uncached model")?;
    assert!(uncached.configured);
    assert_eq!(
        uncached.context_window, None,
        "an uncached model id has no context limit to join"
    );
    assert_eq!(uncached.max_output, None);
    assert_eq!(
        uncached.cost, None,
        "an uncached model id has no price to join"
    );
    assert_eq!(uncached.display, "ollama-cloud / deepseek-v4.1-pro");

    assert!(
        options
            .iter()
            .all(|o| o.provider_display_name != "Ollama Cloud"),
        "the cache section's display name must never label a picker group"
    );
    Ok(())
}

// endregion: --- runnable models per block
