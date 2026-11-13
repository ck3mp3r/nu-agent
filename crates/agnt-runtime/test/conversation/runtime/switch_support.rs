//! Shared fixtures for the switch tests.
//!
//! These build a provider state that can actually switch models, which means a
//! startup plugin config. The resolved provider is a name the models cache
//! lists, so a switch changes the config without reaching a transport: the model
//! builds at its next use, and only the test that drives that build constructs a
//! client.

use std::collections::HashMap;

use super::harness::*;
use super::*;
use crate::config::PluginConfig;
use crate::config::models_cache::{ModelLimit, ModelSpec, ProviderSpec};
use crate::conversation::state::provider::ProviderState;

/// One cache row for `model` with `context` as its context limit and, when the
/// row lists one, `input` as its input limit.
pub(super) fn cache_row(model: &str, context: u32, input: Option<u32>) -> (String, ModelSpec) {
    (
        model.to_string(),
        ModelSpec {
            id: model.to_string(),
            name: model.to_string(),
            tool_call: true,
            limit: ModelLimit {
                context,
                output: 500,
                input,
            },
            cost: None,
            modalities: None,
        },
    )
}

/// A cache listing two `ollama` rows with different context limits, so a
/// switch between them changes the active model's context window.
pub(super) fn two_limit_cache() -> crate::config::ModelsCache {
    crate::config::ModelsCache {
        providers: HashMap::from([(
            "ollama".to_string(),
            ProviderSpec {
                id: "ollama".to_string(),
                name: "ollama".to_string(),
                env: Vec::new(),
                api: None,
                models: HashMap::from([
                    cache_row("small", 1_000, None),
                    cache_row("large", 2_000, None),
                ]),
            },
        )]),
    }
}

/// The startup plugin config that carries [`two_limit_cache`].
pub(super) fn two_limit_plugin_config() -> PluginConfig {
    PluginConfig {
        models_cache: Some(two_limit_cache()),
        ..PluginConfig::default()
    }
}

/// A runtime whose provider resolves model limits from [`two_limit_cache`].
///
/// The provider carries the startup plugin config, so `ModelSwitching`'s
/// `switch_model` resolves and applies a new model. The provider has no cached
/// client: a switch leaves the model stale, and only a test that drives the
/// model's next use asks for a client.
pub(super) fn fixture_with_two_limits() -> Result<RuntimeFixture> {
    let mut fixture = make_runtime(text_model("unused"))?;
    fixture.runtime.provider =
        ProviderState::new(runtime_config(), Some(two_limit_plugin_config()));
    Ok(fixture)
}

/// Record `consumed` tokens as the meter's last reported context count.
///
/// The model-switch gate reads that count. A `None` count records nothing, so
/// the gate reads no reported count — the unknown-count case.
pub(super) fn set_consumed(fixture: &RuntimeFixture, consumed: Option<u64>) {
    if let Some(consumed) = consumed {
        fixture
            .runtime
            .meter
            .record_usage(rig::completion::Usage::new().input_tokens(consumed));
    }
}
