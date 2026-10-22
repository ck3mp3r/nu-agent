//! Post-loop dispatch tests: the terminal arms of `TurnExecutor::dispatch_result`.
//!
//! The `UnknownTool` arm is unreachable through the full turn pipeline: the hook
//! chain answers every invalid tool call with `InvalidToolCallAction::retry`. The
//! arm is therefore driven directly with the terminal value the loop would produce.
//!
//! The suite is split into topical sub-files. Shared imports and the fixture
//! builders live here; each sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "executor_dispatch/completed.rs"]
mod completed;

// endregion: --- Modules

use super::executor_test_support::*;
use super::test_utils::{BusEventCollector, test_config};
use super::*;
use crate::conversation::turn::error::TurnContext;
use crate::conversation::turn::{TurnError, TurnResult};
use crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX;
use crate::protocol::event::UiEvent;
use crate::types::Message;

/// A `TurnResult` for a completed, non-cancelled turn.
fn completed_result(text: &str) -> TurnResult {
    TurnResult {
        text: text.to_string(),
        usage: rig::completion::request::Usage::default(),
        messages: None,
        tool_call_count: 0,
        deltas_emitted: true,
        cancelled: false,
        cancel_reason: None,
        last_total_tokens: 12,
        last_known_history: Vec::new(),
    }
}

/// A `TurnResult` for a cancelled turn.
fn cancelled_result(
    messages: Option<Vec<Message>>,
    last_known_history: Vec<Message>,
    cancel_reason: Option<String>,
    text: &str,
) -> TurnResult {
    TurnResult {
        text: text.to_string(),
        usage: rig::completion::request::Usage::default(),
        messages,
        tool_call_count: 0,
        deltas_emitted: true,
        cancelled: true,
        cancel_reason,
        last_total_tokens: 0,
        last_known_history,
    }
}

fn empty_context() -> TurnContext {
    TurnContext {
        last_known_history: Vec::new(),
    }
}

#[tokio::test]
async fn dispatch_unknown_tool_persists_history_and_reports_failure() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-dispatch-unknown-tool";
    let mut memory_state = make_memory_state(&temp_dir);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(crate::bus::create_bus()),
    );
    let messages = vec![Message::user("use a tool")];

    // -- Exec
    let error = executor
        .dispatch_result(
            Err((
                TurnError::UnknownTool {
                    msg: "Unknown tool: nope".to_string(),
                    tool_name: "nope".to_string(),
                    messages: messages.clone(),
                },
                empty_context(),
            )),
            nu_protocol::Span::test_data(),
            Some(session_id),
        )
        .await
        .err()
        .ok_or("an unknown tool must fail the turn")?;

    // -- Check
    assert!(
        error.to_string().contains("unknown tool"),
        "the error must name the unknown tool; got: {error}"
    );
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert!(
        persisted.is_empty(),
        "a failed turn must save nothing to the rig store; got {persisted:?}"
    );
    Ok(())
}

#[tokio::test]
async fn dispatch_unknown_tool_without_a_session_skips_persistence() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let error = executor
        .dispatch_result(
            Err((
                TurnError::UnknownTool {
                    msg: "Unknown tool: nope".to_string(),
                    tool_name: "nope".to_string(),
                    messages: vec![Message::user("use a tool")],
                },
                empty_context(),
            )),
            nu_protocol::Span::test_data(),
            None,
        )
        .await
        .err()
        .ok_or("an unknown tool must fail the turn")?;

    // -- Check
    assert!(error.to_string().contains("unknown tool"));
    Ok(())
}

#[tokio::test]
async fn dispatch_tool_execution_failure_reports_the_raw_message() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-dispatch-tool-failure";
    let mut memory_state = make_memory_state(&temp_dir);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let error = executor
        .dispatch_result(
            Err((
                TurnError::ToolExecutionFailed {
                    msg: "tool server exploded".to_string(),
                },
                empty_context(),
            )),
            nu_protocol::Span::test_data(),
            Some(session_id),
        )
        .await
        .err()
        .ok_or("a tool execution failure must fail the turn")?;

    // -- Check
    assert_eq!(
        error.to_string(),
        "Turn failed: tool server exploded",
        "a tool failure has no error kind, so the raw message is surfaced"
    );
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert!(
        persisted.is_empty(),
        "a tool failure must save nothing to the rig store; got {persisted:?}"
    );
    Ok(())
}

#[tokio::test]
async fn dispatch_path_c_cancelled_with_an_empty_delta_persists_nothing() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-dispatch-path-c-empty";
    let mut memory_state = make_memory_state(&temp_dir);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let outcome = executor
        .dispatch_result(
            Ok(cancelled_result(Some(Vec::new()), Vec::new(), None, "")),
            nu_protocol::Span::test_data(),
            Some(session_id),
        )
        .await
        .map_err(|e| format!("a cancelled turn must not be an error: {e}"))?;

    // -- Check
    assert!(matches!(outcome, TurnOutcome::EarlyReturn(_)));
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert!(
        persisted.is_empty(),
        "an empty cancelled delta must not write to the session"
    );
    Ok(())
}

#[tokio::test]
async fn dispatch_path_b_uses_the_last_known_history() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-dispatch-path-b-history";
    let mut memory_state = make_memory_state(&temp_dir);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let outcome = executor
        .dispatch_result(
            Ok(cancelled_result(
                None,
                vec![Message::user("completed work")],
                None,
                "",
            )),
            nu_protocol::Span::test_data(),
            Some(session_id),
        )
        .await
        .map_err(|e| format!("a cancelled turn must not be an error: {e}"))?;

    // -- Check
    assert!(matches!(outcome, TurnOutcome::EarlyReturn(_)));
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert!(
        persisted.is_empty(),
        "a cancelled turn must save nothing to the rig store; got {persisted:?}"
    );
    Ok(())
}

#[tokio::test]
async fn dispatch_path_b_true_fallback_synthesizes_a_placeholder() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-dispatch-path-b-fallback";
    let mut memory_state = make_memory_state(&temp_dir);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(crate::bus::create_bus()),
    );

    // -- Exec
    let outcome = executor
        .dispatch_result(
            Ok(cancelled_result(None, Vec::new(), None, "partial text")),
            nu_protocol::Span::test_data(),
            Some(session_id),
        )
        .await
        .map_err(|e| format!("a cancelled turn must not be an error: {e}"))?;

    // -- Check
    assert!(matches!(outcome, TurnOutcome::EarlyReturn(_)));
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert!(
        persisted.is_empty(),
        "a cancelled turn must save nothing to the rig store; got {persisted:?}"
    );
    Ok(())
}

#[tokio::test]
async fn dispatch_path_c_stop_reason_surfaces_warning_and_completed() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);
    let bus = crate::bus::create_bus();
    let mut collector = BusEventCollector::subscribe(&bus);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(bus),
    );
    let reason = format!("{OUTPUT_REPETITION_STOP_PREFIX} repeated output");

    // -- Exec
    let outcome = executor
        .dispatch_result(
            Ok(cancelled_result(
                Some(Vec::new()),
                Vec::new(),
                Some(reason.clone()),
                "",
            )),
            nu_protocol::Span::test_data(),
            None,
        )
        .await
        .map_err(|e| format!("a cancelled turn must not be an error: {e}"))?;

    // -- Check
    let TurnOutcome::EarlyReturn(value) = outcome else {
        return Err("a cancelled turn must early-return".into());
    };
    assert_eq!(
        crate::utils::value_ext::extract_response_text_from_value(&value),
        reason
    );
    let events = collector.drain();
    assert!(
        events.iter().any(|e| matches!(e, UiEvent::Warning { .. })),
        "path C must surface the stop as a warning; got: {events:?}"
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, UiEvent::Completed { .. })),
        "path C must still emit a completion; got: {events:?}"
    );
    Ok(())
}
