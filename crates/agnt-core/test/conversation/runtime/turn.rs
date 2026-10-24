//! `CoreRuntime::execute_turn` tests driven through the real
//! `AgentConversationRuntime` with a `MockCompletionModel`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nu_protocol::Span;
use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::harness::*;
use super::*;
use crate::protocol::contracts::CoreRuntime;
use crate::utils::value_ext::extract_response_text_from_value;

/// Collect the conversation the rig-memory store holds for a session.
async fn persisted_messages(
    fixture: &RuntimeFixture,
    session_id: &str,
) -> Result<Vec<crate::types::Message>> {
    let store = fixture
        .runtime
        .session
        .inner_memory()
        .rig_store(session_id)
        .map_err(|e| format!("rig store open should succeed: {e:?}"))?;
    let conversation = rig_memory::load(&*store, session_id)
        .map_err(|e| format!("rig store load should succeed: {e:?}"))?;
    Ok(conversation
        .messages()
        .iter()
        .map(|message| (**message).clone())
        .collect())
}

#[tokio::test]
async fn execute_turn_completes_and_persists_the_session() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("hello from the mock"))?;
    let session_id = fixture
        .runtime
        .final_session_id
        .clone()
        .ok_or("fixture must carry a session id")?;

    // -- Exec
    let value = fixture
        .runtime
        .execute_turn(&fixture.bus, "hi".to_string(), None, Span::test_data())
        .await
        .map_err(|e| format!("execute_turn should succeed: {e}"))?;

    // -- Check
    assert_eq!(
        extract_response_text_from_value(&value),
        "hello from the mock"
    );
    let messages = persisted_messages(&fixture, &session_id).await?;
    assert_eq!(
        messages.len(),
        2,
        "a completed turn must persist the user prompt and the assistant reply"
    );
    Ok(())
}

#[tokio::test]
async fn execute_turn_includes_context_in_the_preamble() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("with context"))?;

    // -- Exec
    let value = fixture
        .runtime
        .execute_turn(
            &fixture.bus,
            "hi".to_string(),
            Some("extra context block".to_string()),
            Span::test_data(),
        )
        .await
        .map_err(|e| format!("execute_turn with context should succeed: {e}"))?;

    // -- Check
    assert_eq!(extract_response_text_from_value(&value), "with context");
    Ok(())
}

#[tokio::test]
async fn execute_turn_without_a_session_returns_an_unstamped_response() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("sessionless"))?;
    fixture.runtime.final_session_id = None;

    // -- Exec
    let value = fixture
        .runtime
        .execute_turn(&fixture.bus, "hi".to_string(), None, Span::test_data())
        .await
        .map_err(|e| format!("sessionless execute_turn should succeed: {e}"))?;

    // -- Check
    assert_eq!(extract_response_text_from_value(&value), "sessionless");
    let record = value
        .as_record()
        .map_err(|e| format!("response should be a record: {e}"))?;
    let meta = record
        .get("_meta")
        .and_then(|m| m.as_record().ok())
        .ok_or("every response carries a _meta record")?;
    assert!(
        meta.get("session_id").is_none(),
        "a sessionless turn must not stamp a session id"
    );
    assert!(
        meta.get("message_count").is_none(),
        "a sessionless turn must not stamp message_count metadata"
    );
    Ok(())
}

#[tokio::test]
async fn execute_turn_surfaces_a_provider_failure() -> Result<()> {
    // -- Setup & Fixtures
    let model = MockCompletionModel::from_stream_turns([vec![MockStreamEvent::error(
        "500 api_error internal server error",
    )]]);
    let mut fixture = make_runtime(model)?;
    let session_id = fixture
        .runtime
        .final_session_id
        .clone()
        .ok_or("fixture must carry a session id")?;

    // -- Exec
    let error = fixture
        .runtime
        .execute_turn(&fixture.bus, "hi".to_string(), None, Span::test_data())
        .await
        .err()
        .ok_or("a provider failure must surface as an error")?;

    // -- Check
    assert!(
        error.to_string().contains("Turn failed"),
        "the error must be user-facing; got: {error}"
    );
    let messages = persisted_messages(&fixture, &session_id).await?;
    assert_eq!(
        messages.len(),
        1,
        "a failed turn persists the prompt it never answered; got: {messages:?}"
    );
    Ok(())
}

#[tokio::test]
async fn execute_turn_uses_the_interactive_resolver_when_pending_is_set() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("interactive"))?;
    fixture.runtime.interactive_pending = Some(Arc::new(Mutex::new(HashMap::new())));
    fixture.runtime.renders_tool_previews = true;

    // -- Exec
    let value = fixture
        .runtime
        .execute_turn(&fixture.bus, "hi".to_string(), None, Span::test_data())
        .await
        .map_err(|e| format!("interactive execute_turn should succeed: {e}"))?;

    // -- Check
    assert_eq!(extract_response_text_from_value(&value), "interactive");
    Ok(())
}
