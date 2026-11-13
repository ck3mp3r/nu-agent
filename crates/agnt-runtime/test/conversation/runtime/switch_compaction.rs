//! The compactor follows the active model, at the model's next use.
//!
//! A switch marks the shared model stale and builds nothing, so the compactor's
//! wire and planning spec still describe the model the session started on. The
//! next use builds the model, and with it the compactor's wire and a spec
//! re-resolved for the model the build landed on.
//!
//! Integration-gated: the next use is what constructs the provider client.

use super::switch_support::*;
use super::*;
use crate::protocol::model_switching::ModelSwitching;

/// A persona whose front matter selects `ollama/large`.
const LARGE_PERSONA: &str = r#"---
name: large-agent
model: ollama/large
---
You are the large-context agent.
"#;

/// WHEN a switch is followed by the model's next use, THEN the compactor's wire
/// and planning spec SHALL both describe the model the switch landed on, and
/// neither SHALL move before that use.
#[tokio::test]
async fn compaction_model_and_spec_follow_the_switch_at_the_next_use() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = fixture_with_two_limits()?;
    fixture.write_persona("large-agent", LARGE_PERSONA)?;
    let before_spec = fixture.runtime.compaction.spec();
    let before_model_id = fixture.runtime.compaction.model().id().map(str::to_string);

    // -- Exec
    fixture
        .runtime
        .switch_model("ollama/small")
        .await
        .map_err(|e| format!("the model switch should succeed: {e}"))?;
    let stale_model_id = fixture.runtime.compaction.model().id().map(str::to_string);
    let stale_spec = fixture.runtime.compaction.spec();
    fixture
        .runtime
        .ensure_active_model()
        .await
        .map_err(|e| format!("the next use should build the model: {e}"))?;
    let after_model_switch_id = fixture.runtime.compaction.model().id().map(str::to_string);
    let after_model_switch_spec = fixture.runtime.compaction.spec();

    // -- Check: nothing moves in the switch, both follow at the next use.
    assert_eq!(
        stale_model_id, before_model_id,
        "a switch must not rebuild the compactor's wire"
    );
    assert_eq!(
        stale_spec, before_spec,
        "a switch must not re-resolve the planning spec"
    );
    assert_ne!(
        after_model_switch_id, before_model_id,
        "the compactor must not keep the startup wire past the next use"
    );
    assert_eq!(
        after_model_switch_id,
        Some("small".to_string()),
        "the next use reaches the compactor's wire"
    );
    assert_eq!(
        after_model_switch_spec.and_then(|spec| spec.context_window),
        Some(1_000),
        "the planning spec follows the switched-to model's window"
    );

    // -- Exec: a persona switch is the same debt.
    fixture
        .runtime
        .switch_agent("large-agent")
        .await
        .map_err(|e| format!("the persona switch should succeed: {e}"))?;
    fixture
        .runtime
        .ensure_active_model()
        .await
        .map_err(|e| format!("the next use should build the persona's model: {e}"))?;

    // -- Check
    assert_eq!(
        fixture.runtime.compaction.model().id().map(str::to_string),
        Some("large".to_string()),
        "a persona switch reaches the compactor's wire at its next use too"
    );
    assert_eq!(
        fixture
            .runtime
            .compaction
            .spec()
            .and_then(|spec| spec.context_window),
        Some(2_000),
        "the persona model's own window decides the next plan"
    );
    Ok(())
}
