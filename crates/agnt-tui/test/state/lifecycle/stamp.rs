//!
//! The submit boundary and the live carry: a prompt is stamped once, when it is
//! accepted, and its row holds exactly what its carrier kept.

use super::*;
use crate::state::PromptFacts;
use agnt_render::transcript::entry::TranscriptEntry;

/// WHEN the boundary stamps a submission, THE facts SHALL be the instant of the
/// stamp and the identities active at it.
#[test]
fn test_lifecycle_submit_facts_stamps_the_boundary() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.status.identity.active_model_identity = "anthropic/claude-sonnet-4-5".to_string();
    state.status.identity.active_agent_identity = Some("coder".to_string());
    let before = chrono::Utc::now().timestamp_millis();

    // -- Exec
    let facts = state.submit_facts();

    // -- Check
    let after = chrono::Utc::now().timestamp_millis();
    let millis = facts
        .millis
        .ok_or("the boundary always stamps an instant")?;
    assert!(
        (before..=after).contains(&millis),
        "the stamp is the submit instant: {millis} outside [{before}, {after}]"
    );
    assert_eq!(facts.model.as_deref(), Some("anthropic/claude-sonnet-4-5"));
    assert_eq!(facts.agent.as_deref(), Some("coder"));
    Ok(())
}

/// WHEN a live submission reaches the enqueue, THE row its activation pushes
/// SHALL carry the carrier's facts verbatim.
#[test]
fn test_lifecycle_live_enqueue_carries_its_facts_onto_the_row() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    let facts = PromptFacts {
        millis: Some(1_765_000_000_000),
        model: Some("openai/gpt-5".to_string()),
        agent: None,
    };
    state.enqueue_prompt_with_facts("a live prompt".to_string(), facts.clone());

    // -- Exec
    let _ = state.activate_next_prompt();

    // -- Check
    let entry = state
        .transcript
        .entries()
        .last()
        .ok_or("the activation must push a row")?;
    let TranscriptEntry::Message(message) = entry else {
        return Err("the row must be prose".into());
    };
    assert_eq!(message.markdown, "a live prompt");
    assert_eq!(message.timestamp_millis, facts.millis);
    assert_eq!(message.model, facts.model);
    assert_eq!(message.agent, None, "an absent fact stays absent");
    Ok(())
}

/// WHEN queued prompts coalesce, THE row SHALL hold the first `Some` per field
/// in queue order — never a later value, never a fill.
#[test]
fn test_lifecycle_coalesced_row_takes_the_first_some_per_field() -> Result<()> {
    // -- Setup & Fixtures: the first prompt knows the instant and the agent; the
    // second is the only one that knows a model.
    let mut state = AppState::default();
    state.enqueue_prompt_with_facts(
        "one".to_string(),
        PromptFacts {
            millis: Some(1_765_000_000_000),
            model: None,
            agent: Some("coder".to_string()),
        },
    );
    state.enqueue_prompt_with_facts(
        "two".to_string(),
        PromptFacts {
            millis: Some(1_766_000_000_000),
            model: Some("openai/gpt-5".to_string()),
            agent: Some("other".to_string()),
        },
    );

    // -- Exec
    let text = state.take_next_prompt_for_execution();

    // -- Check
    assert_eq!(text.as_deref(), Some("one\ntwo"));
    let entry = state
        .transcript
        .entries()
        .last()
        .ok_or("the coalesce must push a row")?;
    let TranscriptEntry::Message(message) = entry else {
        return Err("the row must be prose".into());
    };
    assert_eq!(
        message.timestamp_millis,
        Some(1_765_000_000_000),
        "first instant wins"
    );
    assert_eq!(
        message.model.as_deref(),
        Some("openai/gpt-5"),
        "the first Some model wins"
    );
    assert_eq!(
        message.agent.as_deref(),
        Some("coder"),
        "the first Some agent wins"
    );
    Ok(())
}

/// WHEN a carrier holds an absent field, THE pushed row SHALL hold that field
/// absent too — the push invents nothing, in either direction.
#[test]
fn test_lifecycle_live_enqueue_keeps_each_absent_field_absent() -> Result<()> {
    // -- Setup & Fixtures: a carrier that knew only the agent.
    let mut state = AppState::default();
    let facts = PromptFacts {
        millis: None,
        model: None,
        agent: Some("coder".to_string()),
    };
    state.enqueue_prompt_with_facts("a bare prompt".to_string(), facts.clone());

    // -- Exec
    let _ = state.activate_next_prompt();

    // -- Check
    let entry = state
        .transcript
        .entries()
        .last()
        .ok_or("the activation must push a row")?;
    let TranscriptEntry::Message(message) = entry else {
        return Err("the row must be prose".into());
    };
    assert_eq!(message.markdown, "a bare prompt");
    assert_eq!(message.timestamp_millis, None, "no invented instant");
    assert_eq!(message.model, None, "no invented model");
    assert_eq!(
        message.agent.as_deref(),
        Some("coder"),
        "the known fact carries"
    );
    Ok(())
}

/// WHEN a prompt takes the immediate lane, THE lane SHALL push no row at all:
/// it runs a command, and its facts stop at the carrier.
#[test]
fn test_lifecycle_immediate_lane_pushes_no_row_and_stops_its_facts() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    state.enqueue_immediate_submission_with_facts(
        "/compact".to_string(),
        PromptFacts {
            millis: Some(1_765_000_000_000),
            model: Some("openai/gpt-5".to_string()),
            agent: Some("coder".to_string()),
        },
    );

    // -- Exec
    let text = state.take_next_prompt_for_execution();

    // -- Check: the command is taken, and the transcript stays untouched.
    assert_eq!(text.as_deref(), Some("/compact"));
    assert!(
        state.transcript.entries().is_empty(),
        "the immediate lane pushes no row: a slash command leaves the transcript untouched"
    );
    Ok(())
}
