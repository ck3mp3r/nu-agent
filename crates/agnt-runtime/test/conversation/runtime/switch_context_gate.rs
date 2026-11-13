//! Session-context gate tests for `AgentRuntime`'s switches.
//!
//! A switch resolves the target config, applies the consumed-token gate, and
//! marks the shared model stale. None of that asks a provider for a client, so
//! the gate is decided here without touching a transport.
//!
//! The one switch path that does build a model — the model's next use — is
//! covered by `switch_compaction`, which is integration-gated for that reason.

use super::switch_support::*;
use super::*;
use crate::protocol::model_switching::ModelSwitching;

/// A persona whose front matter selects `ollama/small` and sets a temperature.
const SMALL_PERSONA: &str = r#"---
name: small-agent
model: ollama/small
temperature: 0.3
---
You are the small-context agent.
"#;

/// A switch to a model the session does not fit must fail with the actionable
/// message and leave the config on the previous model.
#[tokio::test]
async fn switch_model_blocks_a_model_the_session_does_not_fit() -> Result<()> {
    // -- Setup & Fixtures: 5_000 consumed against a 1_000-token window.
    let mut fixture = fixture_with_two_limits()?;
    set_consumed(&fixture, Some(5_000));

    // -- Exec
    let error = fixture
        .runtime
        .switch_model("ollama/small")
        .await
        .err()
        .ok_or("a switch the session does not fit must fail")?;

    // -- Check
    assert_eq!(
        error,
        "cannot switch to 'ollama/small': the session has consumed about 5000 tokens and the \
         model's context window is 1000 tokens; compact the session first (/compact)"
    );
    assert_eq!(
        fixture.runtime.provider_name(),
        "mock",
        "a blocked switch must leave the previous provider in place"
    );
    assert_eq!(
        fixture.runtime.model(),
        "mock-model",
        "a blocked switch must leave the previous model in place"
    );
    Ok(())
}

/// A switch the session fits must succeed and change the model.
#[tokio::test]
async fn switch_model_allows_a_model_the_session_fits() -> Result<()> {
    // -- Setup & Fixtures: 500 consumed against a 1_000-token window.
    let mut fixture = fixture_with_two_limits()?;
    set_consumed(&fixture, Some(500));

    // -- Exec
    let (identity, _) = fixture
        .runtime
        .switch_model("ollama/small")
        .await
        .map_err(|e| format!("a switch the session fits must succeed: {e}"))?;

    // -- Check
    assert_eq!(identity, "ollama/small");
    assert_eq!(fixture.runtime.model(), "small");
    Ok(())
}

/// An unknown consumed count means there is nothing to protect, so the switch
/// proceeds.
#[tokio::test]
async fn switch_model_allows_a_switch_when_the_consumed_count_is_unknown() -> Result<()> {
    // -- Setup & Fixtures: the meter has reported no context count.
    let mut fixture = fixture_with_two_limits()?;

    // -- Exec
    let (identity, _) = fixture
        .runtime
        .switch_model("ollama/small")
        .await
        .map_err(|e| format!("an unknown consumed count must not block the switch: {e}"))?;

    // -- Check
    assert_eq!(identity, "ollama/small");
    assert_eq!(fixture.runtime.model(), "small");
    Ok(())
}

/// A persona whose model the session does not fit keeps the previous model,
/// applies the persona's other settings, and does not fail.
#[tokio::test]
async fn switch_agent_skips_the_model_rewrite_when_the_session_does_not_fit() -> Result<()> {
    // -- Setup & Fixtures: 5_000 consumed against a 1_000-token window.
    let mut fixture = fixture_with_two_limits()?;
    fixture.write_persona("small-agent", SMALL_PERSONA)?;
    set_consumed(&fixture, Some(5_000));

    // -- Exec
    let identity = fixture
        .runtime
        .switch_agent("small-agent")
        .await
        .map_err(|e| format!("a blocked persona model must not fail the switch: {e}"))?;

    // -- Check
    assert_eq!(identity, "small-agent");
    assert_eq!(
        fixture.runtime.model(),
        "mock-model",
        "a blocked persona model must leave the previous model in place"
    );
    assert_eq!(
        fixture.runtime.provider.provider_config().temperature,
        Some(0.3),
        "the persona's other settings must still apply"
    );
    Ok(())
}
