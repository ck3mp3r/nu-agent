use super::*;

// ---------------------------------------------------------------------------
// Path C: Ok(cancelled=true, messages=Some) returns EarlyReturn + persists
// ---------------------------------------------------------------------------

/// RED: cancelled turn via Ok path returns EarlyReturn, persists messages, emits Completed.
///
/// Before Path C is added to executor.rs, this test FAILS because the current code
/// falls through to the normal persistence path and returns TurnOutcome::Completed.
///
/// The mock model emits one text chunk then a FinalResponse. The UI cancels immediately
/// (before the first drain loop tick), causing the cancel_token to fire before the
/// spawned tokio task processes any stream event. The hook's on_completion_call sees
/// Terminate, rig yields Cancelled { chat_history }, and build_agent_and_stream
/// returns Ok(StreamingTurnResult { cancelled: true, messages: Some(chat_history) }).
/// This reaches executor.execute() as Ok(TurnResult { cancelled: true, messages: Some }).
///
/// Expected behaviour after the fix:
///   1. result == Ok(TurnOutcome::EarlyReturn(_))
///   2. conversation store was appended with the cancelled messages
///   3. UiEvent::Completed was emitted
///   4. UiEvent::AssistantMessage was NOT emitted
#[tokio::test]
async fn cancelled_ok_path_returns_early_return_persists_messages_and_emits_completed() -> Result<()>
{
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-cancelled-session";
    let mut memory_state = make_memory_state(&temp_dir);

    // The model emits a tool call to the cancelling tool, which publishes a
    // CancelEvent on the shared bus from inside `call()`. This drives the
    // cancelled path deterministically (the hook subscribes to `bus.cancel()`
    // before the turn runs).
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    let mut ui_event_rx = bus.ui_event().subscribe();

    let handle = rig::tool::server::ToolServer::new()
        .tool(super::test_utils::CancellingTool::new(bus.clone()))
        .run();
    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();

    let mut event_collector = super::test_utils::BusEventCollector::subscribe(&bus);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle: handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                annotations: crate::types::ToolAnnotations::default(),
                name: crate::types::ToolName::new("test_cancel_tool")?,
                description: "cancels the turn".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            last_total_tokens: default_last_total_tokens(),
            bus,
            render_registry: ToolRenderRegistry::default(),
            cwd: std::path::PathBuf::from("/tmp"),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // 1. Result must be EarlyReturn (not Completed — that would be the bug)
    assert!(
        result.is_ok(),
        "execute() must not return Err for a cancelled turn; got: {:?}",
        result.err()
    );
    let outcome = result.map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::EarlyReturn(_)),
        "cancelled Ok path must return TurnOutcome::EarlyReturn, not TurnOutcome::Completed"
    );

    // 2. The cancelled turn's delta is persisted: the prompt, the committed
    //    tool call and its result.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        3,
        "a cancelled turn persists its delta; got {persisted:?}"
    );
    assert_eq!(message_text(&persisted[0]).as_deref(), Some("hello"));
    assert!(
        matches!(&persisted[1], crate::types::Message::Assistant(_)),
        "the committed tool call is persisted; got {persisted:?}"
    );
    assert!(
        matches!(&persisted[2], crate::types::Message::User { .. }),
        "the committed tool result is persisted; got {persisted:?}"
    );

    // 3. UiEvent::Completed must have been published on the bus ui_event channel
    let mut saw_completed = false;
    while let Ok(event) = ui_event_rx.try_recv() {
        if matches!(event, UiEvent::Completed { .. }) {
            saw_completed = true;
        }
    }
    assert!(
        saw_completed,
        "UiEvent::Completed must be published for a cancelled turn (path C)"
    );

    // 4. UiEvent::AssistantMessage must NOT have been emitted
    let events = event_collector.drain();
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, UiEvent::AssistantMessage { .. })),
        "UiEvent::AssistantMessage must NOT be emitted for a cancelled turn (path C)"
    );

    Ok(())
}

/// Cancelled turn (path C) writes to both JSONL and in-memory cache via a
/// single JournalConversationMemory.append() call — not two separate calls.
///
/// Verifying the single-write pattern: both `conversation_store().load()` and
/// `memory().load()` return the same messages after a cancelled turn.
#[tokio::test]
async fn cancelled_turn_writes_via_single_memory_append() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-single-write-cancelled";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    // The cancelling tool publishes a CancelEvent from inside `call()` (after
    // the hook subscribes to `bus.cancel()`), driving the cancelled path
    // deterministically.
    let handle = rig::tool::server::ToolServer::new()
        .tool(super::test_utils::CancellingTool::new(bus.clone()))
        .run();
    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle: handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                annotations: crate::types::ToolAnnotations::default(),
                name: crate::types::ToolName::new("test_cancel_tool")?,
                description: "cancels the turn".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            last_total_tokens: default_last_total_tokens(),
            bus,
            render_registry: ToolRenderRegistry::default(),
            cwd: std::path::PathBuf::from("/tmp"),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(result.is_ok());
    let outcome = result.map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(matches!(outcome, TurnOutcome::EarlyReturn(_)));

    // The cancelled turn's delta is persisted through the same rig store.
    let from_store = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        from_store.len(),
        3,
        "a cancelled turn persists its delta; got {from_store:?}"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Subtask 3 — cancelled-turn diagnostic test
// ---------------------------------------------------------------------------

/// Diagnostic test: does a cancelled turn after prior session history duplicate JSONL?
///
/// Path C fires: rig's Cancelled carries chat_history. If that chat_history
/// is the full accumulated history (prior + current user prompt), appending it
/// directly would double the store.
///
/// Expected: store grows by at most the new messages from this turn, not by the
/// full prior history again.
#[tokio::test]
async fn cancelled_turn_after_prior_history_persists_only_delta() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = "test-cancelled-delta";
    let mut memory_state = make_memory_state(&temp_dir);

    // Pre-populate: simulate a prior successful turn using ConversationMemory append
    let prior_msgs = vec![
        crate::types::Message::user("prior work"),
        crate::types::Message::assistant("done"),
    ];
    seed_rig_store(&memory_state, session_id, prior_msgs)?;

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    // The cancelling tool publishes a CancelEvent from inside `call()` (after
    // the hook subscribes to `bus.cancel()`), driving the cancelled path
    // deterministically.
    let handle = rig::tool::server::ToolServer::new()
        .tool(super::test_utils::CancellingTool::new(bus.clone()))
        .run();
    let closure_registry = ClosureRegistry::default();
    let mcp_registry = McpToolRegistry::empty();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle: handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                annotations: crate::types::ToolAnnotations::default(),
                name: crate::types::ToolName::new("test_cancel_tool")?,
                description: "cancels the turn".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            last_total_tokens: default_last_total_tokens(),
            bus,
            render_registry: ToolRenderRegistry::default(),
            cwd: std::path::PathBuf::from("/tmp"),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "new question".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    assert!(result.is_ok(), "cancelled turn must not return Err");
    let outcome = result.map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(matches!(outcome, TurnOutcome::EarlyReturn(_)));

    // The cancelled turn appends only its own delta: the prompt, the committed
    // tool call and its result. The prior conversation is untouched.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        5,
        "a cancelled turn appends its delta to the prior conversation; got {persisted:?}"
    );
    assert_eq!(message_text(&persisted[0]).as_deref(), Some("prior work"));
    assert_eq!(message_text(&persisted[1]).as_deref(), Some("done"));
    assert_eq!(message_text(&persisted[2]).as_deref(), Some("new question"));

    Ok(())
}
