use super::*;

// ---------------------------------------------------------------------------
// Gap 1B: corrupt session healed by repair on load
// ---------------------------------------------------------------------------

/// Verifies that a session corrupted before Gap 1A (user(ToolResult) → user(Text)
/// with no assistant between) is tolerated: it is seeded into the rig store
/// unchanged, rig's pairing pass accepts it when it builds the request, and the
/// next turn succeeds.
#[tokio::test]
async fn journey_corrupt_session_is_tolerated_on_load() -> Result<()> {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let path = temp_dir.path().to_path_buf();
    let session_id = "journey-repair-load";
    let config = test_config();

    use crate::types::{
        AssistantContent, AssistantMessage, CallId, Message, ToolCall, ToolFunction, ToolName,
        ToolResult, ToolResultContent, UserContent,
    };

    // The corrupt pattern: user(ToolResult) immediately followed by user(Text),
    // no assistant between.
    let corrupt_messages: Vec<Message> = vec![
        // user("run something")
        Message::user("run something"),
        // assistant(tool_call tc1 nu__shell)
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
            ToolCall::new(
                CallId::from_wire("tc1"),
                ToolFunction::new(
                    ToolName::new("nu__shell").expect("non-empty tool name"),
                    serde_json::json!({"command": "git pull"}),
                ),
            ),
        )])),
        // user(tool_result tc1) — pure ToolResult message
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("tc1"),
                name: ToolName::new("nu__shell").expect("non-empty tool name"),
                content: vec![ToolResultContent::text("Already up to date.")],
                is_error: false,
            })],
        },
        // user("continue") — immediately after ToolResult, no assistant between them → CORRUPT
        Message::user("continue"),
    ];

    // Seed the corrupt history into the session's rig store, as a session
    // written by an earlier run would hold it.
    let mut ms = memory_state_at(path.clone());
    {
        let store = ms
            .inner_memory()
            .rig_store(session_id)
            .map_err(|e| format!("rig store open: {e:?}"))?;
        let mut conversation =
            rig_memory::load(&*store, session_id).map_err(|e| format!("rig store load: {e:?}"))?;
        conversation.extend(corrupt_messages.clone());
        rig_memory::save(&*store, session_id, &mut conversation)
            .map_err(|e| format!("rig store save: {e:?}"))?;
    }

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("The git pull succeeded.".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let shared_model = test_utils::shared_model_handle(model.clone());

    let mut executor = TurnExecutor::new(
        &config,
        &mut ms,
        no_tools(),
        shared_model,
        test_utils::test_compaction_config(crate::bus::create_bus()),
    );
    let outcome = executor
        .execute(
            ExecuteInput {
                prompt: "what happened?".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // The turn must succeed — rig tolerates the corrupt history.
    assert!(
        outcome.is_ok(),
        "turn must succeed after the corrupt session is tolerated; got: {outcome:?}"
    );

    // Verify the rig store: the seeded history is the repaired one (the
    // dangling tool result gets a synthetic close and the trailing orphan user
    // message is trimmed), and the new exchange follows.
    let store = ms
        .inner_memory()
        .rig_store(session_id)
        .map_err(|e| format!("rig store open: {e:?}"))?;
    let conversation =
        rig_memory::load(&*store, session_id).map_err(|e| format!("rig store load: {e:?}"))?;
    let saved: Vec<Message> = conversation
        .messages()
        .iter()
        .map(|message| (**message).clone())
        .collect();
    // rig appends a turn's messages to the log; it does not rewrite the
    // history it loaded. The repair is applied to the in-memory history the
    // run used, so the log keeps the seeded prefix and gains the new exchange.
    assert_eq!(
        saved.len(),
        corrupt_messages.len() + 2,
        "the store holds the seeded history and the new exchange; got: {saved:?}"
    );
    assert_eq!(
        &saved[..corrupt_messages.len()],
        corrupt_messages.as_slice(),
        "the seeded prefix is unchanged"
    );
    assert_user_text(&saved[saved.len() - 2], "what happened?");
    assert_assistant_text_contains(&saved[saved.len() - 1], "The git pull succeeded.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Gap 6: empty ToolResult tolerated
// ---------------------------------------------------------------------------

/// Integration test: a session containing a ToolResult with empty content is
/// tolerated: it is seeded into the rig store unchanged, rig accepts it when it
/// builds the request, and the next turn succeeds.
///
/// The store is append-only: the seeded prefix stays as written and the new
/// exchange follows it.
#[tokio::test]
async fn journey_empty_tool_result_is_tolerated() -> Result<()> {
    use crate::types::{
        AssistantContent, AssistantMessage, CallId, ToolCall, ToolFunction, ToolName, ToolResult,
        ToolResultContent, UserContent,
    };

    let mut h = JourneyHarness::new("journey-gap6-empty-tool-result");

    // A valid-structured but empty-content ToolResult message, as a prior turn
    // that stored an empty result would leave it.
    let tc_id = "tc_gap6";
    let prior_messages: Vec<crate::types::Message> = vec![
        crate::types::Message::user("run something"),
        crate::types::Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
            ToolCall::new(
                CallId::from_wire(tc_id),
                ToolFunction::new(ToolName::new("test_echo")?, serde_json::json!({})),
            ),
        )])),
        // Empty tool result — the key scenario for Gap 6
        crate::types::Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire(tc_id),
                name: ToolName::new("test_echo").expect("non-empty tool name"),
                content: vec![ToolResultContent::text("")],
                is_error: false,
            })],
        },
        crate::types::Message::assistant("[ok]"),
    ];

    h.seed_history(prior_messages.clone())?;

    // Now run a follow-up turn — rig must accept the empty ToolResult.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("follow-up answer".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let (r, _) = h.turn("what was the result?", model, no_tools()).await;
    assert!(
        r.is_ok(),
        "turn must succeed with an empty ToolResult in the history; got: {r:?}"
    );

    // The store keeps the seeded prefix and gains the new exchange.
    let saved = h.raw_messages().await?;
    assert_eq!(
        saved.len(),
        prior_messages.len() + 2,
        "the store holds the seeded history and the new exchange; got: {saved:?}"
    );
    assert_eq!(
        &saved[..prior_messages.len()],
        prior_messages.as_slice(),
        "the seeded prefix is unchanged"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Null-args ToolCall repair integration test
// ---------------------------------------------------------------------------

/// Verifies that a session containing a ToolCall whose arguments the model sent
/// as something other than a JSON object (the poison pattern from
/// `on_invalid_tool_call` → Skip → rollback_messages) is tolerated: it is seeded
/// into the rig store unchanged, rig keeps the malformed arguments as text with
/// a `{}` fallback, and the next turn succeeds.
///
/// The store is append-only: the seeded prefix stays as written and the new
/// exchange follows it.
#[tokio::test]
async fn journey_invalid_args_tool_call_repaired_on_load() -> Result<()> {
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let path = temp_dir.path().to_path_buf();
    let session_id = "journey-invalid-args-repair";
    let config = test_config();

    // The corrupt pattern: ToolCall with non-object arguments and its matching
    // ToolResult.
    use crate::types::{
        AssistantContent, AssistantMessage, CallId, Message, ToolCall, ToolFunction, ToolName,
        ToolResult, ToolResultContent, UserContent,
    };

    let corrupt_messages: Vec<Message> = vec![
        // user("run something")
        Message::user("run something"),
        // assistant(tool_call tc1 with non-object arguments) — THE POISON
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::ToolCall(
            ToolCall::new(
                CallId::from_wire("tc_poison"),
                ToolFunction {
                    name: ToolName::new("tmux__send_and_capture").expect("non-empty tool name"),
                    arguments: serde_json::Map::new(),
                    invalid_arguments: Some("not an object".to_string()),
                },
            ),
        )])),
        // user(tool_result tc1 — the Skip reason)
        Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: CallId::from_wire("tc_poison"),
                name: ToolName::new("tmux__send_and_capture").expect("non-empty tool name"),
                content: vec![ToolResultContent::text("Tool not available")],
                is_error: false,
            })],
        },
        // assistant closing text
        Message::assistant("I see the tool is unavailable."),
    ];

    // Seed the poison into the session's rig store, as an earlier run would
    // leave it.
    let mut ms = memory_state_at(path.clone());
    {
        let store = ms
            .inner_memory()
            .rig_store(session_id)
            .map_err(|e| format!("rig store open: {e:?}"))?;
        let mut conversation =
            rig_memory::load(&*store, session_id).map_err(|e| format!("rig store load: {e:?}"))?;
        conversation.extend(corrupt_messages.clone());
        rig_memory::save(&*store, session_id, &mut conversation)
            .map_err(|e| format!("rig store save: {e:?}"))?;
    }

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("The tool was unavailable but we can continue.".into()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let shared_model = test_utils::shared_model_handle(model.clone());

    let mut executor = TurnExecutor::new(
        &config,
        &mut ms,
        no_tools(),
        shared_model,
        test_utils::test_compaction_config(crate::bus::create_bus()),
    );
    let outcome = executor
        .execute(
            ExecuteInput {
                prompt: "what happened?".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // The turn must succeed — rig keeps the malformed arguments as text with a
    // `{}` fallback, so the poison never fails a reply.
    assert!(
        outcome.is_ok(),
        "turn must succeed with an invalid-args ToolCall in the history; got: {outcome:?}"
    );

    // The store keeps the seeded prefix and gains the new exchange.
    let store = ms
        .inner_memory()
        .rig_store(session_id)
        .map_err(|e| format!("rig store open: {e:?}"))?;
    let conversation =
        rig_memory::load(&*store, session_id).map_err(|e| format!("rig store load: {e:?}"))?;
    let saved: Vec<Message> = conversation
        .messages()
        .iter()
        .map(|message| (**message).clone())
        .collect();
    assert_eq!(
        saved.len(),
        corrupt_messages.len() + 2,
        "the store holds the seeded history and the new exchange; got: {saved:?}"
    );
    assert_eq!(
        &saved[..corrupt_messages.len()],
        corrupt_messages.as_slice(),
        "the seeded prefix is unchanged"
    );
    Ok(())
}
