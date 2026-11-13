//! Integration-gated depth tests for `AgentRuntime`.
//!
//! `ModelSwitching::switch_model` resolves the target config and marks the
//! shared model stale; the model's next use builds it, and the provider client
//! with it. That build constructs a real transport client (TLS provider init +
//! `reqwest::Client`), so the test lives behind the `integration` feature.

use super::harness::*;
use super::*;
use crate::config::PluginConfig;
use crate::conversation::state::provider::ProviderState;
use crate::protocol::model_switching::ModelSwitching;

/// WHEN the model's next use follows a switch, THEN that use SHALL build the
/// shared model from the provider the switch resolved, and cache the client it
/// built from.
#[tokio::test]
async fn switch_model_rebuilds_the_shared_model_at_its_next_use() -> Result<()> {
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

    // -- Check: the switch itself builds nothing.
    assert_eq!(identity, "ollama/llama3");
    assert_eq!(fixture.runtime.provider_name(), "ollama");
    assert_eq!(fixture.runtime.model(), "llama3");
    assert_eq!(
        max_tokens,
        fixture.runtime.max_context_tokens(),
        "the returned context limit must match the resolved config"
    );
    assert!(
        fixture.runtime.provider.cached_client().is_none(),
        "the switch must build no client: the model builds at its next use"
    );

    // -- Exec: the next use pays the debt.
    fixture
        .runtime
        .ensure_active_model()
        .await
        .map_err(|e| format!("the next use should build the model: {e}"))?;

    // -- Check
    assert_eq!(
        fixture.runtime.compaction.model().id().map(str::to_string),
        Some("llama3".to_string()),
        "the rebuilt model is the one the switch resolved"
    );
    assert!(
        fixture.runtime.provider.cached_client().is_some(),
        "the next use caches the client it built the model from"
    );
    Ok(())
}
