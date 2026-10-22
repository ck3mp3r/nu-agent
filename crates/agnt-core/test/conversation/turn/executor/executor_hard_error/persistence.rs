use super::*;

// ---------------------------------------------------------------------------
// Error path persistence tests
// ---------------------------------------------------------------------------

/// MaxTurns carries full chat_history, but rig saves only a successful run, so
/// a MaxTurns failure saves nothing.
///
/// Setup: mock returns a tool_call on turn 1. Config limits tool turns to 0, so rig
/// raises MaxTurns after the first tool call attempt.
#[tokio::test]
async fn max_turns_error_saves_nothing() -> Result<()> {
    // max_tool_turns=0: rig raises MaxTurns as soon as a tool-call turn would
    // be scheduled (current_turn > 0 + 1 after the first tool response).
    let config = Config {
        max_tool_turns: Some(0),
        ..test_config()
    };
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-max-turns";
    let mut memory_state = make_memory_state(&temp_dir);

    // Turn 1: model asks for a tool call. With max_turns=0, rig will MaxTurns
    // as soon as it tries to schedule the tool-call turn.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call("tool_call_1", "some_tool", serde_json::json!({"x": 1})),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "please call a tool".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err (it's a hard error, not a cancellation)
    assert!(
        result.is_err(),
        "MaxTurns must propagate as LabeledError to caller"
    );

    // rig saves nothing on a failed run
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert!(
        persisted.is_empty(),
        "a MaxTurns failure must save nothing to the rig store; got {persisted:?}"
    );

    Ok(())
}

/// UnknownToolCall carries full chat_history, but rig saves only a successful
/// run, so an unknown-tool failure saves nothing.
///
/// Setup: mock returns a tool_call for "nonexistent_tool" which is not registered
/// in the agent's tool list. Rig raises UnknownToolCall.
#[tokio::test]
async fn unknown_tool_error_saves_nothing() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-unknown-tool";
    let mut memory_state = make_memory_state(&temp_dir);

    // Model calls a tool that is not registered — triggers UnknownToolCall.
    // No visible_tool_definitions → agent has no tools → any tool call is unknown.
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::tool_call(
            "tool_call_1",
            "nonexistent_tool",
            serde_json::json!({"arg": "value"}),
        ),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "use a tool please".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err
    assert!(
        result.is_err(),
        "UnknownToolCall must propagate as LabeledError to caller"
    );

    // rig saves nothing on a failed run
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert!(
        persisted.is_empty(),
        "an unknown-tool failure must save nothing to the rig store; got {persisted:?}"
    );

    Ok(())
}

/// A network/CompletionError on a fresh session saves nothing: rig saves only a
/// successful run.
#[tokio::test]
async fn network_error_on_fresh_session_saves_nothing() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-network-error";
    let prompt_text = "what is the weather today?";
    let mut memory_state = make_memory_state(&temp_dir);

    // Streaming error on the first event — simulates network failure.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("network timeout")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: prompt_text.to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err
    assert!(
        result.is_err(),
        "network error must propagate as LabeledError to caller"
    );

    // rig saves nothing on a failed run
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert!(
        persisted.is_empty(),
        "a network failure must save nothing to the rig store; got {persisted:?}"
    );

    Ok(())
}

/// A CompletionError on the first LLM call saves nothing: rig saves only a
/// successful run.
#[tokio::test]
async fn hard_error_on_first_llm_call_saves_nothing() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-hard-error-no-hook-history";
    let prompt_text = "fresh turn on empty session";
    let mut memory_state = make_memory_state(&temp_dir);

    // Fresh session — no prior messages. on_completion_call fires with history=[]
    // and prompt = user_msg. After fix: last_known_history = [user_msg].
    // delta = skip(0) = [user_msg] → delta path fires → 1 message persisted.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: prompt_text.to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // Must return Err
    assert!(
        result.is_err(),
        "hard error must propagate as Err to caller"
    );

    // rig saves nothing on a failed run
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert!(
        persisted.is_empty(),
        "a hard error on the first LLM call must save nothing to the rig store; got {persisted:?}"
    );

    Ok(())
}

/// When there is no session (transient invocation), hard errors must NOT write
/// anything to the store — there is no conversation to record.
///
/// No append call exists any more: rig saves only a successful run, and a
/// session-less turn has no rig store at all.
#[tokio::test]
async fn hard_error_no_session_persists_nothing() {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let mut memory_state = make_memory_state(&temp_dir);

    // Streaming error — hard failure, no history recoverable.
    let model =
        MockCompletionModel::from_stream_turns([[MockStreamEvent::error("provider unavailable")]]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let mut executor = make_executor(
        &config,
        &mut memory_state,
        shared_model,
        default_tool_infra(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "a transient prompt".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            None, // <-- no session
        )
        .await;

    // Must return Err
    assert!(result.is_err(), "hard error must propagate as LabeledError");

    // No JSONL should have been written — transient invocation with no session_id
    // There is no specific conversation_id to check, so we verify the temp dir
    // has no .jsonl files written (the store uses conversation_id as filename).
    let jsonl_files: Vec<_> = std::fs::read_dir(temp_dir.path())
        .expect("read_dir should succeed")
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    assert!(
        jsonl_files.is_empty(),
        "no JSONL files must be written for a transient (no-session) hard error; found: {:?}",
        jsonl_files.iter().map(|e| e.path()).collect::<Vec<_>>()
    );
}
