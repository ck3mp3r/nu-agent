//! Integration-gated session-context gate tests for `AgentRuntime`.
//!
//! `ModelSwitching::switch_model` and `switch_agent` rebuild the shared model
//! through the resolved provider, so the success path constructs a real
//! transport client (TLS provider init + `reqwest::Client`). That is
//! integration-level work, so the tests live behind the `integration` feature.

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

/// A persona whose front matter selects `ollama/large`.
const LARGE_PERSONA: &str = r#"---
name: large-agent
model: ollama/large
---
You are the large-context agent.
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
    // -- Setup & Fixtures: `last_total_tokens` starts `None`.
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

/// The compactor is startup-fixed: neither a model switch nor a persona switch
/// may touch its wire or its planning spec.
#[tokio::test]
async fn compaction_model_stays_startup_fixed_across_switches() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = fixture_with_two_limits()?;
    fixture.write_persona("large-agent", LARGE_PERSONA)?;
    let before_spec = fixture.runtime.compaction.spec().cloned();
    let before_model_id = fixture.runtime.compaction.model().id().map(str::to_string);

    // -- Exec
    fixture
        .runtime
        .switch_model("ollama/small")
        .await
        .map_err(|e| format!("the model switch should succeed: {e}"))?;
    let after_model_switch_spec = fixture.runtime.compaction.spec().cloned();
    let after_model_switch_id = fixture.runtime.compaction.model().id().map(str::to_string);
    fixture
        .runtime
        .switch_agent("large-agent")
        .await
        .map_err(|e| format!("the persona switch should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        after_model_switch_spec, before_spec,
        "a model switch must not touch the planning spec"
    );
    assert_eq!(
        after_model_switch_id, before_model_id,
        "a model switch must not touch the compactor's wire"
    );
    assert_eq!(
        fixture.runtime.compaction.spec().cloned(),
        before_spec,
        "a persona switch must not touch the planning spec"
    );
    assert_eq!(
        fixture.runtime.compaction.model().id().map(str::to_string),
        before_model_id,
        "a persona switch must not touch the compactor's wire"
    );
    Ok(())
}
