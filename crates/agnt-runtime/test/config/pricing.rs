//! The pricing ladder: the alias table, every rung, and the sweep.
//!
//! The suite is split into topical sub-files. Shared imports and fixtures live
//! here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "pricing/invariant.rs"]
mod invariant;
#[path = "pricing/ladder.rs"]
mod ladder;
#[path = "pricing/rung4.rs"]
mod rung4;

// endregion: --- Modules

use super::*;

use std::collections::HashMap;

use crate::config::models_cache::{ModelLimit, ProviderSpec};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// One fixture row: provider key, model id, and its cost when priced.
type Row<'a> = (&'a str, &'a str, Option<(f64, f64)>);

/// One expected sweep outcome: provider key, model id, rung, and rates.
type Expected<'a> = (&'a str, &'a str, LadderRung, Option<(f64, f64)>);

// region:    --- Test Support

fn model_spec(model_id: &str, cost: Option<(f64, f64)>) -> ModelSpec {
    ModelSpec {
        id: model_id.to_string(),
        name: model_id.to_string(),
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
        modalities: None,
    }
}

fn cache_of(rows: &[Row<'_>]) -> ModelsCache {
    let mut providers: HashMap<String, ProviderSpec> = HashMap::new();
    for (provider, model, cost) in rows {
        let entry = providers
            .entry((*provider).to_string())
            .or_insert_with(|| ProviderSpec {
                id: (*provider).to_string(),
                name: (*provider).to_string(),
                env: Vec::new(),
                api: None,
                models: HashMap::new(),
            });
        entry
            .models
            .insert((*model).to_string(), model_spec(model, *cost));
    }
    ModelsCache { providers }
}

/// The fixture mirrors the real duplication classes: a copilot row with no
/// cost whose id openai prices, an ollama row with no cost and no other
/// vendor's row, and own-priced rows.
fn fixture_cache() -> ModelsCache {
    cache_of(&[
        ("github-copilot", "gpt-4o", None),
        ("github-copilot", "claude-sonnet-4.6", Some((3.0, 15.0))),
        ("openai", "gpt-4o", Some((2.5, 10.0))),
        ("anthropic", "claude-sonnet-4-6", Some((3.0, 15.0))),
        ("ollama-cloud", "llama3.3", None),
    ])
}

fn rates_of(pricing: Option<Pricing>) -> Option<(f64, f64)> {
    pricing.map(|pricing| (pricing.input, pricing.output))
}

/// A configured price with no cache rates, as the config record parses one.
fn config_price(input: f64, output: f64) -> ModelCost {
    ModelCost {
        input,
        output,
        cache_read: None,
        cache_write: None,
    }
}

// endregion: --- Test Support
