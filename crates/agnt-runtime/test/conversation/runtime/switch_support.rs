//! Shared fixtures for the integration-gated switch tests.
//!
//! These helpers need a provider state that can actually switch models, which
//! means a startup plugin config and a resolved client — integration-level
//! work. They live in their own module so the unit build, where no test
//! consumes them, never sees them as dead code.

use std::collections::HashMap;

use super::harness::*;
use super::*;
use crate::config::PluginConfig;
use crate::config::models_cache::{ModelLimit, ModelSpec, ProviderSpec};
use crate::conversation::state::provider::ProviderState;

/// One cache row for `model` with `context` as its context limit.
pub(super) fn cache_row(model: &str, context: u32) -> (String, ModelSpec) {
    (
        model.to_string(),
        ModelSpec {
            id: model.to_string(),
            name: model.to_string(),
            tool_call: true,
            limit: ModelLimit {
                context,
                output: 500,
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
                models: HashMap::from([cache_row("small", 1_000), cache_row("large", 2_000)]),
            },
        )]),
    }
}

/// A runtime whose provider resolves model limits from [`two_limit_cache`].
///
/// The provider carries the startup plugin config, so `ModelSwitching`'s
/// `switch_model` resolves and applies a new model. Ollama needs no
/// credentials, so the resolved provider builds a client without touching a
/// keychain or a network.
pub(super) fn fixture_with_two_limits() -> Result<RuntimeFixture> {
    let mut fixture = make_runtime(text_model("unused"))?;
    let plugin_config = PluginConfig {
        models_cache: Some(two_limit_cache()),
        ..PluginConfig::default()
    };
    fixture.runtime.provider = ProviderState::new(runtime_config(), Some(plugin_config));
    Ok(fixture)
}

/// Record `consumed` tokens as the session's last completion count.
pub(super) fn set_consumed(fixture: &RuntimeFixture, consumed: Option<u64>) {
    *fixture
        .runtime
        .last_total_tokens
        .lock()
        .expect("model mutex poisoned") = consumed;
}
