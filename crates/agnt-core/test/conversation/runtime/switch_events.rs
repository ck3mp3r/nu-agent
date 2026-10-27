//! Integration-gated switch-event publish tests for `AgentRuntime`.
//!
//! The publishes live in `ModelSwitching::switch_model` and `switch_agent`, so
//! proving them needs a runtime whose provider can actually switch — the
//! `fixture_with_two_limits` harness. The ACP session harness cannot reach
//! these sites: its provider state has no startup plugin config, so
//! `switch_model` is unavailable there.

use super::harness::*;
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

/// A persona whose front matter selects `ollama/small`, a model the session
/// cannot fit once it has consumed more than 1_000 tokens.
const SMALL_PERSONA: &str = r#"---
name: small-agent
model: ollama/small
---
You are the small-context agent.
"#;

/// A persona whose front matter names a model the resolver rejects: a bare
/// role name is not a `provider/model` specification, so the model switch the
/// persona asks for fails.
const UNRESOLVABLE_PERSONA: &str = r#"---
name: broken-agent
model: no-such-role
---
You are the unresolvable agent.
"#;

/// One switch event, flattened for comparison: `(event, from, to)`.
type Switch = (String, Option<String>, String);

/// The switch events currently buffered on the fixture's session channel.
fn take_switches(fixture: &RuntimeFixture, rx: &mut crate::bus::SessionRx) -> Vec<Switch> {
    let _ = fixture;
    let mut switches = Vec::new();
    loop {
        match rx.try_recv() {
            Ok(crate::bus::SessionEvent::AgentSwitched { from, to }) => {
                switches.push(("agent_switched".to_string(), from, to));
            }
            Ok(crate::bus::SessionEvent::ModelSwitched { from, to }) => {
                switches.push(("model_switched".to_string(), Some(from), to));
            }
            Ok(_) => {}
            Err(crate::bus::TryRecvError::Lagged(_)) => continue,
            Err(_) => break,
        }
    }
    switches
}

/// A successful model switch publishes `ModelSwitched` with the identity it
/// left and the one it landed on, after the envelope stamp moved.
#[tokio::test]
async fn switch_model_publishes_model_switched() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = fixture_with_two_limits()?;
    let mut rx = fixture.bus.session().subscribe();

    // -- Exec
    fixture
        .runtime
        .switch_model("ollama/small")
        .await
        .map_err(|e| format!("the switch to the small model should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        take_switches(&fixture, &mut rx),
        vec![(
            "model_switched".to_string(),
            Some("mock/mock-model".to_string()),
            "ollama/small".to_string()
        )],
        "a successful model switch must publish exactly one model_switched"
    );
    assert_eq!(
        fixture.runtime.hook_context.model(),
        "ollama/small",
        "the publish must follow the envelope stamp, so a hook sees the new model"
    );
    Ok(())
}

/// A persona switch whose model override takes effect publishes both facts,
/// agent first, each stamped from the post-switch state.
#[tokio::test]
async fn switch_agent_with_a_model_override_publishes_agent_then_model() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = fixture_with_two_limits()?;
    fixture.write_persona("large-agent", LARGE_PERSONA)?;
    let mut rx = fixture.bus.session().subscribe();

    // -- Exec
    fixture
        .runtime
        .switch_agent("large-agent")
        .await
        .map_err(|e| format!("the switch to the large-context persona should succeed: {e}"))?;

    // -- Check: two facts changed, so two events, agent first.
    assert_eq!(
        take_switches(&fixture, &mut rx),
        vec![
            (
                "agent_switched".to_string(),
                None,
                "large-agent".to_string()
            ),
            (
                "model_switched".to_string(),
                Some("mock/mock-model".to_string()),
                "ollama/large".to_string()
            ),
        ],
        "a persona model override that takes effect publishes both events in order"
    );
    assert_eq!(
        fixture.runtime.hook_context.persona().as_deref(),
        Some("large-agent"),
        "the agent envelope must carry the new slug"
    );
    assert_eq!(
        fixture.runtime.hook_context.model(),
        "ollama/large",
        "the model envelope must carry the overridden model"
    );
    Ok(())
}

/// A persona model the provider rejects publishes the agent switch only: the
/// model never changed, so there is no model switch to report.
#[tokio::test]
async fn switch_agent_with_a_rejected_model_override_publishes_agent_only() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = fixture_with_two_limits()?;
    fixture.write_persona("broken-agent", UNRESOLVABLE_PERSONA)?;
    let mut rx = fixture.bus.session().subscribe();

    // -- Exec
    fixture
        .runtime
        .switch_agent("broken-agent")
        .await
        .map_err(|e| format!("the persona switch itself must succeed: {e}"))?;

    // -- Check
    assert_eq!(
        take_switches(&fixture, &mut rx),
        vec![(
            "agent_switched".to_string(),
            None,
            "broken-agent".to_string()
        )],
        "a rejected model override must not publish model_switched"
    );
    assert_eq!(
        fixture.runtime.model(),
        "mock-model",
        "the rejected override must leave the previous model in place"
    );
    Ok(())
}

/// A persona model the context-window gate rolls back publishes the agent
/// switch only.
#[tokio::test]
async fn switch_agent_with_a_rolled_back_model_override_publishes_agent_only() -> Result<()> {
    // -- Setup & Fixtures: 5_000 consumed against the persona's 1_000 window.
    let mut fixture = fixture_with_two_limits()?;
    fixture.write_persona("small-agent", SMALL_PERSONA)?;
    set_consumed(&fixture, Some(5_000));
    let mut rx = fixture.bus.session().subscribe();

    // -- Exec
    fixture
        .runtime
        .switch_agent("small-agent")
        .await
        .map_err(|e| format!("a blocked persona model must not fail the switch: {e}"))?;

    // -- Check
    assert_eq!(
        take_switches(&fixture, &mut rx),
        vec![(
            "agent_switched".to_string(),
            None,
            "small-agent".to_string()
        )],
        "a rolled-back model override must not publish model_switched"
    );
    assert_eq!(
        fixture.runtime.model(),
        "mock-model",
        "the rolled-back override must leave the previous model in place"
    );
    Ok(())
}
