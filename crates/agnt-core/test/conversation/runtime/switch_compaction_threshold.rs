//! Integration-gated compaction-threshold refresh tests for `AgentRuntime`.
//!
//! `ModelSwitching::switch_model` and `switch_agent` rebuild the shared model
//! through the resolved provider, so the success path constructs a real
//! transport client (TLS provider init + `reqwest::Client`). That is
//! integration-level work, so the tests live behind the `integration` feature.

use super::harness::*;
use super::*;
use crate::config::PluginConfig;
use crate::config::models_cache::{ModelLimit, ModelSpec, ProviderSpec};
use crate::conversation::state::provider::ProviderState;
use crate::protocol::model_switching::ModelSwitching;

/// A persona whose front matter selects `ollama/small`.
const SMALL_PERSONA: &str = r#"---
name: small-agent
model: ollama/small
---
You are the small-context agent.
"#;

/// A persona whose front matter selects `ollama/large`.
const LARGE_PERSONA: &str = r#"---
name: large-agent
model: ollama/large
---
You are the large-context agent.
"#;

/// One cache row for `model` with `context` as its context limit.
fn cache_row(model: &str, context: u32) -> (String, ModelSpec) {
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
fn two_limit_cache() -> crate::config::ModelsCache {
    crate::config::ModelsCache {
        providers: std::collections::HashMap::from([(
            "ollama".to_string(),
            ProviderSpec {
                id: "ollama".to_string(),
                name: "ollama".to_string(),
                env: Vec::new(),
                api: None,
                models: std::collections::HashMap::from([
                    cache_row("small", 1_000),
                    cache_row("large", 2_000),
                ]),
            },
        )]),
    }
}

/// A runtime whose provider resolves model limits from `two_limit_cache`.
///
/// Ollama needs no credentials, so the resolved provider builds a client
/// without touching a keychain or a network.
fn fixture_with_two_limits() -> Result<RuntimeFixture> {
    let mut fixture = make_runtime(text_model("unused"))?;
    let plugin_config = PluginConfig {
        models_cache: Some(two_limit_cache()),
        ..PluginConfig::default()
    };
    fixture.runtime.provider = ProviderState::new(runtime_config(), Some(plugin_config));
    Ok(fixture)
}

/// A model switch must recompute the compaction threshold against the new
/// model's context window: 0.8 of 1_000, then 0.8 of 2_000.
#[tokio::test]
async fn switch_model_refreshes_the_compaction_threshold() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = fixture_with_two_limits()?;

    // -- Exec
    fixture
        .runtime
        .switch_model("ollama/small")
        .await
        .map_err(|e| format!("the switch to the small model should succeed: {e}"))?;
    let small = fixture.runtime.compaction.threshold_tokens;
    fixture
        .runtime
        .switch_model("ollama/large")
        .await
        .map_err(|e| format!("the switch to the large model should succeed: {e}"))?;
    let large = fixture.runtime.compaction.threshold_tokens;

    // -- Check
    assert_eq!(
        small,
        Some(800),
        "the threshold must follow the small model's 1_000-token context"
    );
    assert_eq!(
        large,
        Some(1_600),
        "the threshold must follow the large model's 2_000-token context"
    );
    Ok(())
}

/// A persona switch that applies a model must refresh the threshold the same
/// way.
#[tokio::test]
async fn switch_agent_refreshes_the_compaction_threshold() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = fixture_with_two_limits()?;
    fixture.write_persona("small-agent", SMALL_PERSONA)?;
    fixture.write_persona("large-agent", LARGE_PERSONA)?;

    // -- Exec
    fixture
        .runtime
        .switch_agent("small-agent")
        .await
        .map_err(|e| format!("the switch to the small-context persona should succeed: {e}"))?;
    let small = fixture.runtime.compaction.threshold_tokens;
    fixture
        .runtime
        .switch_agent("large-agent")
        .await
        .map_err(|e| format!("the switch to the large-context persona should succeed: {e}"))?;
    let large = fixture.runtime.compaction.threshold_tokens;

    // -- Check
    assert_eq!(
        small,
        Some(800),
        "the persona's small model must set the threshold from its 1_000-token context"
    );
    assert_eq!(
        large,
        Some(1_600),
        "the persona's large model must set the threshold from its 2_000-token context"
    );
    Ok(())
}
