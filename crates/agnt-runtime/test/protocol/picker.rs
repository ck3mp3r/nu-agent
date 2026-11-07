use super::*;

// region:    --- Modules

#[path = "picker/config_pricing.rs"]
mod config_pricing;

#[path = "picker/declared_models.rs"]
mod declared_models;

// endregion: --- Modules

use std::collections::HashMap;

use crate::config::models_cache::{ModelCost, ModelLimit, ModelSpec, ProviderSpec};
use crate::config::{ModelConfig, ProviderConfig};
use crate::protocol::persona::PersonaSummary;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

type CostedModelFixture<'a> = (&'a str, &'a str, u32, u32, Option<(f64, f64)>);

// region:    --- Test Support

fn provider_config(models: &[&str]) -> ProviderConfig {
    let mut map = HashMap::new();
    for model in models {
        map.insert((*model).to_string(), ModelConfig::default());
    }
    ProviderConfig {
        models: map,
        ..ProviderConfig::default()
    }
}

fn plugin_config(providers: &[(&str, &[&str])]) -> PluginConfig {
    let mut map = HashMap::new();
    for (id, models) in providers {
        map.insert((*id).to_string(), provider_config(models));
    }
    PluginConfig {
        providers: map,
        ..PluginConfig::default()
    }
}

/// A plugin config whose single block declares the cache section it serves.
fn plugin_config_with_catalog(provider: &str, catalog: &str, models: &[&str]) -> PluginConfig {
    let mut map = HashMap::new();
    map.insert(
        provider.to_string(),
        ProviderConfig {
            catalog: Some(catalog.to_string()),
            ..provider_config(models)
        },
    );
    PluginConfig {
        providers: map,
        ..PluginConfig::default()
    }
}

fn provider_spec(id: &str, name: &str, models: &[(&str, &str, u32, u32)]) -> ProviderSpec {
    let mut map = HashMap::new();
    for (model_id, model_name, context, output) in models {
        map.insert(
            (*model_id).to_string(),
            ModelSpec {
                id: (*model_id).to_string(),
                name: (*model_name).to_string(),
                tool_call: true,
                limit: ModelLimit {
                    context: *context,
                    output: *output,
                },
                cost: None,
                modalities: None,
            },
        );
    }
    ProviderSpec {
        id: id.to_string(),
        name: name.to_string(),
        env: Vec::new(),
        api: None,
        models: map,
    }
}

fn provider_spec_with_cost(
    id: &str,
    name: &str,
    models: &[CostedModelFixture<'_>],
) -> ProviderSpec {
    let mut map = HashMap::new();
    for (model_id, model_name, context, output, cost) in models {
        map.insert(
            (*model_id).to_string(),
            ModelSpec {
                id: (*model_id).to_string(),
                name: (*model_name).to_string(),
                tool_call: true,
                limit: ModelLimit {
                    context: *context,
                    output: *output,
                },
                cost: cost.map(|(input, output)| ModelCost {
                    input,
                    output,
                    cache_read: None,
                    cache_write: None,
                }),
                modalities: None,
            },
        );
    }
    ProviderSpec {
        id: id.to_string(),
        name: name.to_string(),
        env: Vec::new(),
        api: None,
        models: map,
    }
}

fn models_cache(providers: &[ProviderSpec]) -> ModelsCache {
    let mut map = HashMap::new();
    for spec in providers {
        map.insert(spec.id.clone(), spec.clone());
    }
    ModelsCache { providers: map }
}

fn persona_summary(name: &str, description: Option<&str>, builtin: bool) -> PersonaSummary {
    PersonaSummary {
        name: name.to_string(),
        description: description.map(str::to_string),
        builtin,
    }
}

// endregion: --- Test Support

// region:    --- format_active_model_identity

#[test]
fn test_picker_format_active_model_identity_prefixes_bare_model() {
    // -- Setup & Fixtures
    let provider = "ollama";
    let model = "llama3";

    // -- Exec
    let identity = format_active_model_identity(provider, model);

    // -- Check
    assert_eq!(identity, "ollama/llama3");
}

#[test]
fn test_picker_format_active_model_identity_keeps_prefixed_model() {
    // -- Setup & Fixtures
    let provider = "ollama";
    let model = "ollama/llama3";

    // -- Exec
    let identity = format_active_model_identity(provider, model);

    // -- Check
    assert_eq!(identity, "ollama/llama3");
}

// endregion: --- format_active_model_identity

// region:    --- build_model_picker_catalog

#[test]
fn test_picker_build_model_picker_catalog_without_cache_lists_configured_models() -> Result<()> {
    // -- Setup & Fixtures
    let config = plugin_config(&[("zeta", &["b", "a"]), ("alpha", &["m"])]);

    // -- Exec
    let options = build_model_picker_catalog(None, &config, "alpha/m");

    // -- Check
    let identities: Vec<&str> = options.iter().map(|o| o.identity.as_str()).collect();
    assert_eq!(identities, vec!["alpha/m", "zeta/a", "zeta/b"]);

    let first = options.first().ok_or("catalog must not be empty")?;
    assert!(first.active, "the active identity must be flagged");
    assert!(first.configured, "a configured model must be flagged");
    assert_eq!(first.display, "alpha / m");
    assert_eq!(first.provider_display_name, "alpha");
    assert_eq!(first.context_window, None);
    assert_eq!(first.max_output, None);

    let second = options.get(1).ok_or("catalog must hold zeta/a")?;
    assert!(!second.active);
    assert_eq!(second.display, "zeta / a");
    Ok(())
}

#[test]
fn test_picker_build_model_picker_catalog_with_cache_lists_declared_models_and_limits() -> Result<()>
{
    // -- Setup & Fixtures: the cache files ollama's models under
    // `ollama-cloud`, as models.dev does, and lists a model the block does
    // not declare. The block declares that section, so it owns it.
    let cache = models_cache(&[provider_spec(
        "ollama-cloud",
        "Ollama",
        &[
            ("llama3", "Llama 3", 8192, 2048),
            ("mistral", "Mistral", 4096, 1024),
        ],
    )]);
    let config = plugin_config_with_catalog("ollama", "ollama-cloud", &["llama3"]);

    // -- Exec
    let options = build_model_picker_catalog(Some(&cache), &config, "ollama/llama3");

    // -- Check
    assert_eq!(
        options.len(),
        2,
        "a block that declares the section lists every row in it"
    );

    let declared = options.first().ok_or("catalog must hold llama3")?;
    assert_eq!(declared.model, "llama3");
    assert!(declared.configured);
    assert!(declared.active);
    assert_eq!(declared.display, "ollama / Llama 3");
    assert_eq!(declared.provider_display_name, "ollama");
    assert_eq!(declared.context_window, Some(8192));
    assert_eq!(declared.max_output, Some(2048));

    let undeclared = options.get(1).ok_or("catalog must hold mistral")?;
    assert_eq!(undeclared.model, "mistral");
    assert!(!undeclared.configured);
    assert_eq!(undeclared.context_window, Some(4096));
    Ok(())
}

#[test]
fn test_picker_build_model_picker_catalog_cache_missing_provider_falls_back() -> Result<()> {
    // -- Setup & Fixtures
    let cache = models_cache(&[provider_spec(
        "openai",
        "OpenAI",
        &[("gpt-4", "GPT-4", 128000, 4096)],
    )]);
    let config = plugin_config(&[("ollama", &["llama3"])]);

    // -- Exec
    let options = build_model_picker_catalog(Some(&cache), &config, "ollama/llama3");

    // -- Check
    assert_eq!(options.len(), 1);
    let option = options.first().ok_or("catalog must hold llama3")?;
    assert_eq!(option.identity, "ollama/llama3");
    assert_eq!(option.display, "ollama / llama3");
    assert_eq!(option.provider_display_name, "ollama");
    assert_eq!(option.context_window, None);
    assert!(option.configured);
    Ok(())
}

#[test]
fn test_picker_build_model_picker_catalog_sorts_configured_first() -> Result<()> {
    // -- Setup & Fixtures: the block owns its cache section, so the section
    // row it does not declare is listed after the declared one.
    let cache = models_cache(&[provider_spec(
        "ollama-cloud",
        "Ollama Cloud",
        &[("aaa", "A", 1, 1), ("zzz", "Z", 1, 1)],
    )]);
    let config = plugin_config(&[("ollama-cloud", &["zzz"])]);

    // -- Exec
    let options = build_model_picker_catalog(Some(&cache), &config, "ollama-cloud/zzz");

    // -- Check
    let models: Vec<&str> = options.iter().map(|o| o.model.as_str()).collect();
    assert_eq!(
        models,
        vec!["zzz", "aaa"],
        "the declared model must sort before the undeclared section row"
    );
    let undeclared = options
        .get(1)
        .ok_or("catalog must hold the undeclared row")?;
    assert!(!undeclared.configured);
    Ok(())
}

#[test]
fn test_picker_build_model_picker_catalog_with_cost_formats_price() -> Result<()> {
    // -- Setup & Fixtures: the block declares the section it serves.
    let cache = models_cache(&[provider_spec_with_cost(
        "ollama-cloud",
        "Ollama",
        &[("llama3", "Llama 3", 8192, 2048, Some((3.0, 15.0)))],
    )]);
    let config = plugin_config_with_catalog("ollama", "ollama-cloud", &["llama3"]);

    // -- Exec
    let options = build_model_picker_catalog(Some(&cache), &config, "ollama/llama3");

    // -- Check
    let option = options.first().ok_or("catalog must hold llama3")?;
    assert_eq!(option.cost.as_deref(), Some("$3.00/$15.00"));
    Ok(())
}

#[test]
fn test_picker_build_model_picker_catalog_without_cost_is_none() -> Result<()> {
    // -- Setup & Fixtures
    let cache = models_cache(&[provider_spec_with_cost(
        "ollama-cloud",
        "Ollama",
        &[("llama3", "Llama 3", 8192, 2048, None)],
    )]);
    let config = plugin_config(&[("ollama", &["llama3"])]);

    // -- Exec
    let options = build_model_picker_catalog(Some(&cache), &config, "ollama/llama3");

    // -- Check
    let option = options.first().ok_or("catalog must hold llama3")?;
    assert_eq!(option.cost, None);
    Ok(())
}

#[test]
fn test_picker_build_model_picker_catalog_cost_column_does_not_borrow_another_vendors_rates()
-> Result<()> {
    // -- Setup & Fixtures: the block declares the `github-copilot` section,
    // whose gpt-4o row carries no cost; openai prices the same id. No
    // borrowing happens, so the cost column stays empty.
    let cache = models_cache(&[
        provider_spec_with_cost(
            "github-copilot",
            "GitHub Copilot",
            &[("gpt-4o", "GPT-4o", 128_000, 16_384, None)],
        ),
        provider_spec_with_cost(
            "openai",
            "OpenAI",
            &[("gpt-4o", "GPT-4o", 128_000, 16_384, Some((2.5, 10.0)))],
        ),
    ]);
    let config = plugin_config_with_catalog("copilot", "github-copilot", &["gpt-4o"]);

    // -- Exec
    let options = build_model_picker_catalog(Some(&cache), &config, "copilot/gpt-4o");

    // -- Check
    let option = options.first().ok_or("catalog must hold copilot gpt-4o")?;
    assert_eq!(option.identity, "copilot/gpt-4o");
    assert_eq!(
        option.cost, None,
        "the cost column must not borrow another vendor's rates"
    );
    Ok(())
}

#[test]
fn test_picker_build_model_picker_catalog_undeclared_catalog_lists_declared_models() -> Result<()> {
    // -- Setup & Fixtures: the config names the provider `copilot` and declares
    // no catalog; the cache files copilot's models under `github-copilot` and
    // lists a model the block does not declare.
    let cache = models_cache(&[provider_spec_with_cost(
        "github-copilot",
        "GitHub Copilot",
        &[
            (
                "claude-sonnet-4.6",
                "Claude Sonnet 4.6",
                200_000,
                64_000,
                Some((3.0, 15.0)),
            ),
            ("gpt-4o", "GPT-4o", 128_000, 16_384, Some((2.5, 10.0))),
        ],
    )]);
    let config = plugin_config(&[("copilot", &["claude-sonnet-4.6"])]);

    // -- Exec
    let options = build_model_picker_catalog(Some(&cache), &config, "copilot/claude-sonnet-4.6");

    // -- Check
    assert_eq!(
        options.len(),
        1,
        "a block with no declared catalog lists only what it declares"
    );
    let option = options.first().ok_or("catalog must hold the model")?;
    assert_eq!(option.identity, "copilot/claude-sonnet-4.6");
    assert_eq!(option.display, "copilot / claude-sonnet-4.6");
    assert_eq!(option.provider_display_name, "copilot");
    assert_eq!(
        option.context_window, None,
        "a block with no declared catalog joins no metadata"
    );
    assert!(option.configured);
    assert!(option.active);
    Ok(())
}

// endregion: --- build_model_picker_catalog

// region:    --- model_picker_catalog_from_cached_startup_plugin_config

#[test]
fn test_picker_model_picker_catalog_from_cached_startup_plugin_config_none_is_empty() {
    // -- Setup & Fixtures
    let startup_plugin_config: Option<&PluginConfig> = None;

    // -- Exec
    let options = model_picker_catalog_from_cached_startup_plugin_config(
        startup_plugin_config,
        "ollama/llama3",
    );

    // -- Check
    assert!(options.is_empty());
}

#[test]
fn test_picker_model_picker_catalog_from_cached_startup_plugin_config_some_delegates() -> Result<()>
{
    // -- Setup & Fixtures
    let config = plugin_config(&[("ollama", &["llama3"])]);

    // -- Exec
    let options =
        model_picker_catalog_from_cached_startup_plugin_config(Some(&config), "ollama/llama3");

    // -- Check
    assert_eq!(options.len(), 1);
    let option = options.first().ok_or("catalog must hold llama3")?;
    assert_eq!(option.identity, "ollama/llama3");
    assert!(option.active);
    Ok(())
}

// endregion: --- model_picker_catalog_from_cached_startup_plugin_config

// region:    --- build_agent_picker_catalog

#[test]
fn test_picker_build_agent_picker_catalog_display_variants() -> Result<()> {
    // -- Setup & Fixtures
    let agents = vec![
        persona_summary("planner", Some("plans the work"), true),
        persona_summary("maker", None, true),
        persona_summary("custom", Some("a custom agent"), false),
        persona_summary("bare", None, false),
    ];

    // -- Exec
    let options = build_agent_picker_catalog(&agents);

    // -- Check
    let displays: Vec<&str> = options.iter().map(|o| o.display.as_str()).collect();
    assert_eq!(
        displays,
        vec![
            "planner \u{2014} plans the work [built-in]",
            "maker [built-in]",
            "custom \u{2014} a custom agent",
            "bare",
        ]
    );

    let planner = options.first().ok_or("catalog must hold planner")?;
    assert_eq!(planner.name, "planner");
    assert_eq!(planner.description.as_deref(), Some("plans the work"));
    assert!(planner.builtin);

    let bare = options.get(3).ok_or("catalog must hold bare")?;
    assert_eq!(bare.description, None);
    assert!(!bare.builtin);
    Ok(())
}

#[test]
fn test_picker_build_agent_picker_catalog_empty_input_is_empty() {
    // -- Setup & Fixtures
    let agents: Vec<PersonaSummary> = Vec::new();

    // -- Exec
    let options = build_agent_picker_catalog(&agents);

    // -- Check
    assert!(options.is_empty());
}

// endregion: --- build_agent_picker_catalog
