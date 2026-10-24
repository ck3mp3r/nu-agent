//! Depth tests for the `AgentRuntime` trait surface that the other sub-files
//! leave uncovered: the `execute_turn` early-return branch, the persona
//! `additional_params` overlay, the `ModelSwitching` trait accessors, the
//! rig-store splice `run_compaction` persists, and the `load_session`
//! contains-match fallback.

use nu_protocol::Span;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::harness::*;
use super::*;
use crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX;
use crate::protocol::contracts::CoreRuntime;
use crate::protocol::model_switching::ModelSwitching;
use crate::protocol::session_management::SessionPersistence;
use crate::types::Message;
use crate::utils::value_ext::extract_response_text_from_value;

/// A persona whose front matter carries an `additional_params` mapping.
const PARAMS_PERSONA: &str = r#"---
name: params-agent
additional_params:
  top_p: 0.5
  seed: 7
---
You are the params agent.
"#;

/// Seed the rig-memory store of `id` with `messages`.
async fn seed_session(fixture: &RuntimeFixture, id: &str, messages: &[Message]) -> Result<()> {
    let store = fixture
        .runtime
        .session
        .inner_memory()
        .rig_store(id)
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, id)?;
    conversation.extend(messages.iter().cloned());
    rig_memory::save(&*store, id, &mut conversation)?;
    Ok(())
}

/// A turn that repeats one text-only completion forever, so the cross-turn
/// repetition ladder escalates to Stop and the executor converts the stop into
/// steering retries until the per-turn cap is reached.
fn repeating_model() -> MockCompletionModel {
    let turn = vec![
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::final_response_with_total_tokens(1),
    ];
    MockCompletionModel::from_stream_turns((0..60).map(|_| turn.clone()))
}

#[tokio::test]
async fn execute_turn_returns_early_when_the_repetition_ladder_stops_at_the_cap() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(repeating_model())?;

    // -- Exec
    // The ladder is session-scoped and shared across turns, so the stop is only
    // reached after several identical turns. Drive turns until the executor
    // surfaces the terminal stop instead of a completed response.
    let mut stop_text = None;
    for _ in 0..10 {
        let value = fixture
            .runtime
            .execute_turn(&fixture.bus, "hello".to_string(), None, Span::test_data())
            .await
            .map_err(|e| format!("execute_turn should succeed: {e}"))?;
        let text = extract_response_text_from_value(&value);
        if text.starts_with(OUTPUT_REPETITION_STOP_PREFIX) {
            stop_text = Some(text);
            break;
        }
    }

    // -- Check
    let text = stop_text.ok_or("the repetition ladder must reach the stop cap")?;
    assert!(
        text.starts_with(OUTPUT_REPETITION_STOP_PREFIX),
        "the early return must carry the stop reason; got: {text}"
    );
    Ok(())
}

#[tokio::test]
async fn switch_agent_applies_additional_params_from_the_persona() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    fixture.write_persona("params-agent", PARAMS_PERSONA)?;
    assert_eq!(
        fixture.runtime.provider.provider_config().additional_params,
        None,
        "the fixture must start without additional params"
    );

    // -- Exec
    fixture
        .runtime
        .switch_agent("params-agent")
        .await
        .map_err(|e| format!("switch_agent should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        fixture.runtime.provider.provider_config().additional_params,
        Some(serde_json::json!({"top_p": 0.5, "seed": 7})),
        "the persona's additional_params must replace the runtime config value"
    );
    Ok(())
}

#[tokio::test]
async fn model_switching_trait_accessors_read_the_provider_and_persona() {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused")).expect("fixture");
    fixture
        .runtime
        .provider
        .provider_config_mut()
        .max_context_tokens = Some(321_000);

    // -- Exec & Check
    assert_eq!(
        ModelSwitching::max_context_tokens(&fixture.runtime),
        Some(321_000),
        "the trait accessor must widen the configured context limit"
    );
    assert_eq!(
        ModelSwitching::agent_description(&fixture.runtime),
        Some("test description")
    );
    assert_eq!(ModelSwitching::agent_icon(&fixture.runtime), Some("robot"));
}

#[tokio::test]
async fn run_compaction_splices_the_rig_store() -> Result<()> {
    // -- Setup & Fixtures
    // The conversation lives in the rig-memory store, which is what a run loads.
    let mut fixture = make_runtime(text_model("unused"))?;
    let store = fixture
        .runtime
        .session
        .inner_memory()
        .rig_store("test-session")
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, "test-session")?;
    conversation.extend([
        Message::user("first"),
        Message::assistant("reply"),
        Message::user("second"),
        Message::assistant("second reply"),
    ]);
    rig_memory::save(&*store, "test-session", &mut conversation)?;

    // -- Exec
    fixture
        .runtime
        .run_compaction("slash")
        .await
        .map_err(|e| format!("compaction should succeed: {e}"))?;

    // -- Check
    // The splice is persisted to the store, which is what the next run loads.
    let stored = rig_memory::load(&*store, "test-session")?;
    let stored: Vec<Message> = stored
        .messages()
        .iter()
        .map(|message| (**message).clone())
        .collect();
    assert_eq!(
        stored.len(),
        2,
        "the compacted context must hold the summary and the newest message; got: {stored:?}"
    );
    let summary = stored.first().ok_or("the compacted context is non-empty")?;
    assert!(
        matches!(summary, Message::User { .. }),
        "the splice must lead with the summary user message, got: {summary:?}"
    );
    Ok(())
}

#[tokio::test]
async fn load_session_resolves_a_contains_match() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    seed_session(&fixture, "abc-mid-xyz", &[Message::user("contains match")]).await?;

    // -- Exec
    // `mid` is neither the full id nor a suffix of it, so only the contains
    // fallback can resolve the session.
    let transcript = fixture
        .runtime
        .load_session("mid")
        .await
        .map_err(|e| format!("contains load_session should succeed: {e}"))?;

    // -- Check
    assert_eq!(fixture.runtime.current_session_id(), Some("abc-mid-xyz"));
    assert_eq!(transcript.len(), 1);
    assert_eq!(transcript[0].content(), "contains match");
    Ok(())
}
