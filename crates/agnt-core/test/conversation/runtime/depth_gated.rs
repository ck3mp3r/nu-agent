//! Integration-gated depth tests for `AgentRuntime`.
//!
//! `ModelSwitching::switch_model` invalidates the cached provider client and
//! rebuilds the shared model through the resolved provider, so the success path
//! constructs a real transport client (TLS provider init + `reqwest::Client`).
//! That is integration-level work, so the test lives behind the `integration`
//! feature.

use super::harness::*;
use super::*;
use crate::config::PluginConfig;
use crate::conversation::state::provider::ProviderState;
use crate::protocol::model_switching::ModelSwitching;

#[tokio::test]
async fn switch_model_rebuilds_the_shared_model_from_the_resolved_provider() -> Result<()> {
    // -- Setup & Fixtures
    // Ollama needs no credentials, so the resolved provider builds a client
    // without touching a keychain or a network.
    let mut fixture = make_runtime(text_model("unused"))?;
    fixture.runtime.provider = ProviderState::new(runtime_config(), Some(PluginConfig::default()));

    // -- Exec
    let (identity, max_tokens) = fixture
        .runtime
        .switch_model("ollama/llama3")
        .await
        .map_err(|e| format!("switch_model should succeed: {e}"))?;

    // -- Check
    assert_eq!(identity, "ollama/llama3");
    assert_eq!(fixture.runtime.provider_name(), "ollama");
    assert_eq!(fixture.runtime.model(), "llama3");
    assert_eq!(
        max_tokens,
        fixture.runtime.max_context_tokens(),
        "the returned context limit must match the resolved config"
    );
    assert!(
        fixture.runtime.provider.cached_client().is_some(),
        "the switch must cache the client it rebuilt the model from"
    );
    Ok(())
}
