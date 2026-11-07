//! Integration-gated compaction-threshold refresh tests for `AgentRuntime`.
//!
//! `ModelSwitching::switch_model` and `switch_agent` rebuild the shared model
//! through the resolved provider, so the success path constructs a real
//! transport client (TLS provider init + `reqwest::Client`). That is
//! integration-level work, so the tests live behind the `integration` feature.

use super::switch_support::*;
use super::*;
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

/// A persona whose front-matter display name differs from its file slug.
const SLUGGED_PERSONA: &str = r#"---
name: My Agent
model: ollama/large
---
You are the slugged agent.
"#;

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

/// A model switch must refresh the hook context's model cell.
#[tokio::test]
async fn switch_model_refreshes_the_hook_context_model() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = fixture_with_two_limits()?;

    // -- Exec
    fixture
        .runtime
        .switch_model("ollama/small")
        .await
        .map_err(|e| format!("the switch to the small model should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        fixture.runtime.hook_context.model(),
        "ollama/small",
        "the envelope model must follow the switch"
    );
    Ok(())
}

/// A persona switch must refresh BOTH hook context cells: the slug and the
/// model the persona resolves to.
#[tokio::test]
async fn switch_agent_refreshes_both_hook_context_cells() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = fixture_with_two_limits()?;
    fixture.write_persona("slugged", SLUGGED_PERSONA)?;

    // -- Exec
    fixture
        .runtime
        .switch_agent("slugged")
        .await
        .map_err(|e| format!("the switch to the slugged persona should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        fixture.runtime.hook_context.persona().as_deref(),
        Some("slugged"),
        "the envelope agent must be the slug, not the display name"
    );
    assert_eq!(
        fixture.runtime.hook_context.model(),
        "ollama/large",
        "the envelope model must follow the persona's model override"
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
