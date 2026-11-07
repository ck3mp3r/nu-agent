//! `build_dyn_model` binds the pricing ladder into the wire facts.

use super::*;

use std::collections::HashMap;

use crate::config::pricing::PricingView;
use crate::llm::{PricedOutcome, PricingSkip, computed_cost};

// region:    --- Test Support

/// One cache row: its id, its cost when priced, and its input modalities.
fn cache_row(
    model: &str,
    cost: Option<(f64, f64)>,
    modalities: Option<Vec<&str>>,
) -> (String, crate::config::models_cache::ModelSpec) {
    use crate::config::models_cache::{ModelCost, ModelLimit, ModelSpec};

    (
        model.to_string(),
        ModelSpec {
            id: model.to_string(),
            name: model.to_string(),
            tool_call: true,
            limit: ModelLimit {
                context: 128_000,
                output: 4_096,
            },
            cost: cost.map(|(input, output)| ModelCost {
                input,
                output,
                cache_read: None,
                cache_write: None,
            }),
            modalities: modalities.map(|input| crate::config::models_cache::Modalities {
                input: input.into_iter().map(str::to_string).collect(),
                output: vec!["text".to_string()],
            }),
        },
    )
}

fn cache_provider(
    id: &str,
    models: Vec<(String, crate::config::models_cache::ModelSpec)>,
) -> (String, crate::config::models_cache::ProviderSpec) {
    (
        id.to_string(),
        crate::config::models_cache::ProviderSpec {
            id: id.to_string(),
            name: id.to_string(),
            env: Vec::new(),
            api: None,
            models: models.into_iter().collect(),
        },
    )
}

/// A cache whose copilot row for `gpt-4o` carries no cost while openai prices
/// the same id, whose ollama row prices nothing anywhere, and which lists one
/// model rig's built-in catalog does not know.
fn ladder_cache() -> crate::config::ModelsCache {
    use std::collections::HashMap;

    crate::config::ModelsCache {
        providers: HashMap::from([
            cache_provider("github-copilot", vec![cache_row("gpt-4o", None, None)]),
            cache_provider(
                "openai",
                vec![
                    cache_row("gpt-4o", Some((2.5, 10.0)), None),
                    cache_row(
                        "zzz-unlisted-model",
                        Some((1.0, 2.0)),
                        Some(vec!["text", "image"]),
                    ),
                ],
            ),
            cache_provider("ollama-cloud", vec![cache_row("llama3.3", None, None)]),
        ]),
    }
}

/// One cache row: provider key, model id, and its cost when priced.
type Row<'a> = (&'a str, &'a str, Option<(f64, f64)>);

/// A cache holding one row per `(provider, model, cost)` entry.
fn cache_of(rows: &[Row<'_>]) -> crate::config::ModelsCache {
    let mut providers: HashMap<String, crate::config::models_cache::ProviderSpec> = HashMap::new();
    for (provider, model, cost) in rows {
        let entry = providers.entry((*provider).to_string()).or_insert_with(|| {
            crate::config::models_cache::ProviderSpec {
                id: (*provider).to_string(),
                name: (*provider).to_string(),
                env: Vec::new(),
                api: None,
                models: HashMap::new(),
            }
        });
        entry
            .models
            .insert((*model).to_string(), cache_row(model, *cost, None).1);
    }
    crate::config::ModelsCache { providers }
}

/// A Copilot client on a recording transport: no network, no TLS.
fn copilot_client() -> CachedProviderClient {
    CachedProviderClient::Copilot(
        rig::providers::openai::OpenAIConfig::with_key(
            &rig::providers::copilot::DIALECT,
            "tid=test".to_string(),
        )
        .connect(RecordingHttpClient::new("")),
    )
}

/// An OpenAI client on a recording transport: no network, no TLS.
fn openai_client() -> CachedProviderClient {
    CachedProviderClient::OpenAi(
        rig::providers::openai::OpenAIConfig::new("sk-test".to_string())
            .connect(RecordingHttpClient::new("")),
    )
}

/// An OpenAI client on the Chat Completions route, as a `base_url` selects:
/// no network, no TLS.
fn openai_completions_client() -> CachedProviderClient {
    use rig::providers::openai::{OpenAIConfig, Route};

    CachedProviderClient::OpenAiCompletions(
        OpenAIConfig::new("sk-ollama-test".to_string())
            .with_base_url("https://ollama.com/v1")
            .with_route(Route::Chat)
            .connect(RecordingHttpClient::new("")),
    )
}

/// A usage that reports both counters, so a priced spec can price it.
fn priced_usage() -> rig::completion::Usage {
    rig::completion::Usage::new()
        .input_tokens(1_000)
        .output_tokens(500)
        .total_tokens(1_500)
}

/// The priced total and completeness, or an error naming the skip.
fn priced(outcome: PricedOutcome) -> Result<(f64, bool)> {
    match outcome {
        PricedOutcome::Priced { total, complete } => Ok((total, complete)),
        PricedOutcome::Skipped(reason) => {
            Err(format!("expected a price, got a skip: {reason}").into())
        }
    }
}

/// The skip reason, or an error naming the price.
fn skipped(outcome: PricedOutcome) -> Result<PricingSkip> {
    match outcome {
        PricedOutcome::Skipped(reason) => Ok(reason),
        PricedOutcome::Priced { total, .. } => {
            Err(format!("expected a skip, got a price of {total}").into())
        }
    }
}

// endregion: --- Test Support

// region:    --- build_dyn_model

#[test]
fn test_providers_build_dyn_model_does_not_borrow_another_vendors_rates() -> Result<()> {
    // -- Setup & Fixtures: the block is named `copilot` and declares no
    // catalog, so it reaches no cache section. openai prices the same id, but
    // no borrowing happens.
    let cache = ladder_cache();
    let view = PricingView::new(&cache);
    let client = copilot_client();

    // -- Exec
    let model = client.build_dyn_model("gpt-4o", "copilot", Some(&view), None)?;

    // -- Check
    assert_eq!(
        model.name(),
        "copilot",
        "the wire must report the copilot vendor"
    );
    assert_eq!(
        model.spec().and_then(|spec| spec.pricing),
        None,
        "the bound spec must not borrow the openai row's rates"
    );
    assert_eq!(
        skipped(computed_cost(&model, &priced_usage()))?,
        PricingSkip::NoPricing,
        "an unborrowed model must skip with NoPricing"
    );
    Ok(())
}

#[test]
fn test_providers_build_dyn_model_residual_leaves_the_wire_unpriced() -> Result<()> {
    // -- Setup & Fixtures
    let cache = ladder_cache();
    let view = PricingView::new(&cache);
    let client = CachedProviderClient::Ollama(
        rig::providers::ollama::OllamaConfig::new()
            .with_base_url("http://localhost:11434")
            .connect(RecordingHttpClient::new("")),
    );

    // -- Exec
    let model = client.build_dyn_model("llama3.3", "ollama", Some(&view), None)?;

    // -- Check
    assert_eq!(model.name(), "ollama");
    assert_eq!(
        model.spec().and_then(|spec| spec.pricing),
        None,
        "the residual must price nothing"
    );
    assert_eq!(
        skipped(computed_cost(&model, &priced_usage()))?,
        PricingSkip::NoPricing,
        "the residual must skip with NoPricing"
    );
    Ok(())
}

#[test]
fn test_providers_build_dyn_model_unlisted_model_takes_the_cache_modalities() -> Result<()> {
    // -- Setup & Fixtures: a model rig's built-in catalog does not list.
    let cache = ladder_cache();
    let view = PricingView::new(&cache);
    let client = openai_client();

    // -- Exec
    let model = client.build_dyn_model("zzz-unlisted-model", "openai", Some(&view), None)?;

    // -- Check
    let spec = model.spec().ok_or("the bound wire must resolve a spec")?;
    assert!(
        spec.input.image,
        "the cache's image modality must reach the spec"
    );
    assert_eq!(spec.context_window, Some(128_000));
    assert_eq!(spec.max_output_tokens, Some(4_096));
    assert_eq!(spec.pricing.map(|pricing| pricing.input), Some(1.0));
    Ok(())
}

#[test]
fn test_providers_build_dyn_model_without_a_ladder_keeps_the_builtin_catalog() -> Result<()> {
    // -- Setup & Fixtures: no pricing view, so the wire answers from rig's
    // built-in catalog.
    let client = openai_client();

    // -- Exec
    let model = client.build_dyn_model("gpt-4.1", "openai", None, None)?;

    // -- Check
    let pricing = model
        .spec()
        .and_then(|spec| spec.pricing)
        .ok_or("the built-in catalog prices openai gpt-4.1")?;
    assert!(pricing.input > 0.0);
    Ok(())
}

// endregion: --- build_dyn_model

// region:    --- config pricing

#[test]
fn test_providers_build_dyn_model_binds_the_config_price_without_a_cache() -> Result<()> {
    // -- Setup & Fixtures: no models cache at all, only a configured price.
    let config_pricing = crate::config::models_cache::ModelCost {
        input: 0.5,
        output: 1.5,
        cache_read: None,
        cache_write: None,
    };
    let client = CachedProviderClient::Ollama(
        rig::providers::ollama::OllamaConfig::new()
            .with_base_url("http://localhost:11434")
            .connect(RecordingHttpClient::new("")),
    );

    // -- Exec
    let model = client.build_dyn_model("llama3.3", "ollama", None, Some(&config_pricing))?;

    // -- Check
    let pricing = model
        .spec()
        .and_then(|spec| spec.pricing)
        .ok_or("the configured price must reach the bound spec")?;
    assert_eq!((pricing.input, pricing.output), (0.5, 1.5));
    let (total, complete) = priced(computed_cost(&model, &priced_usage()))?;
    let expected = (1_000.0 * 0.5 + 500.0 * 1.5) / 1_000_000.0;
    assert!(
        (total - expected).abs() < 1e-12,
        "the bound spec must price the usage at the configured rates, got {total}"
    );
    assert!(
        complete,
        "a usage with no cache tokens needs no cache rate, so the cost is complete"
    );
    Ok(())
}

#[test]
fn test_providers_build_dyn_model_config_price_beats_the_cache() -> Result<()> {
    // -- Setup & Fixtures: the cache prices openai gpt-4o, and the user
    // declared a different price. The user stated what they pay, so it wins.
    let cache = ladder_cache();
    let view = PricingView::new(&cache);
    let config_pricing = crate::config::models_cache::ModelCost {
        input: 99.0,
        output: 99.0,
        cache_read: None,
        cache_write: None,
    };
    let client = openai_client();

    // -- Exec
    let model = client.build_dyn_model("gpt-4o", "openai", Some(&view), Some(&config_pricing))?;

    // -- Check
    let pricing = model
        .spec()
        .and_then(|spec| spec.pricing)
        .ok_or("the configured price must price the model")?;
    assert_eq!(
        (pricing.input, pricing.output),
        (99.0, 99.0),
        "the configured price must win over the shipped row"
    );
    Ok(())
}

// endregion: --- config pricing

// region:    --- wire vendor ownership

/// An Anthropic client on a recording transport: no network, no TLS.
fn anthropic_client() -> CachedProviderClient {
    CachedProviderClient::Anthropic(
        rig::providers::anthropic::AnthropicConfig::new("sk-ant-test".to_string())
            .connect(RecordingHttpClient::new("")),
    )
}

/// An Ollama client on a recording transport: no network, no TLS.
fn ollama_client() -> CachedProviderClient {
    CachedProviderClient::Ollama(
        rig::providers::ollama::OllamaConfig::new()
            .with_base_url("http://localhost:11434")
            .connect(RecordingHttpClient::new("")),
    )
}

/// Every arm files its facts under the vendor its own wire reports.
///
/// This is the property the deleted per-arm vendor table asserted: if a wire's
/// `describe().name` ever diverged from the vendor the facts were filed under,
/// `ModelFacts::for_model`'s bound-path check would miss and the wire would
/// silently fall back to the built-in catalog.
#[test]
fn test_providers_build_dyn_model_files_facts_under_the_wires_own_vendor() -> Result<()> {
    // -- Setup & Fixtures: one priced row per vendor, so a bound spec resolves.
    let cache = cache_of(&[
        ("github-copilot", "claude-sonnet-4.6", Some((3.0, 15.0))),
        ("openai", "gpt-4o", Some((2.5, 10.0))),
        ("anthropic", "claude-sonnet-4-6", Some((3.0, 15.0))),
        ("ollama-cloud", "gemma4:31b", Some((1.0, 2.0))),
    ]);
    let view = PricingView::new(&cache);
    let cases = [
        (
            "github-copilot",
            copilot_client(),
            "claude-sonnet-4.6",
            "copilot",
        ),
        ("openai", openai_client(), "gpt-4o", "openai"),
        (
            "anthropic",
            anthropic_client(),
            "claude-sonnet-4-6",
            "anthropic",
        ),
        ("ollama-cloud", ollama_client(), "gemma4:31b", "ollama"),
    ];

    // -- Exec & Check
    for (provider, client, model_name, vendor) in cases {
        let model = client.build_dyn_model(model_name, provider, Some(&view), None)?;
        assert_eq!(
            model.name(),
            vendor,
            "the {provider} wire must report its own vendor"
        );
        let pricing = model
            .spec()
            .and_then(|spec| spec.pricing)
            .ok_or_else(|| format!("the {provider} arm must resolve its bound spec"))?;
        assert!(
            pricing.input > 0.0,
            "the {provider} arm must carry the row's prices, not the builtin fallback"
        );
    }
    Ok(())
}

// endregion: --- wire vendor ownership

// region:    --- Modules

#[path = "pricing/repro.rs"]
mod repro;

// endregion: --- Modules
