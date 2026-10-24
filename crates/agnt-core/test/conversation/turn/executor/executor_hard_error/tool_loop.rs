use super::*;

/// A CompletionError on the first LLM call of a fresh session persists the
/// prompt it never answered: rig saves only a successful run.
#[tokio::test]
async fn hard_error_on_first_llm_call_no_prior_history_persists_the_prompt() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-hard-error-fresh-session-2";
    let prompt_text = "fresh turn on empty session";
    let mut memory_state = make_memory_state(&temp_dir);

    // Fresh session — no prior messages. on_completion_call fires with history=[]
    // and prompt = user_msg. After fix: last_known_history = [user_msg].
    // delta = skip(0) = [user_msg] (non-empty) → delta path fires → 1 message.
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

    // The failed turn persists the prompt it never answered.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        1,
        "a hard error on the first LLM call persists the prompt; got {persisted:?}"
    );
    assert_eq!(message_text(&persisted[0]).as_deref(), Some(prompt_text));

    Ok(())
}

// ---------------------------------------------------------------------------
// New test: hard error mid-tool-loop preserves real tool results
// ---------------------------------------------------------------------------

/// A CompletionError on the second LLM sub-call of a multi-tool-call turn
/// persists the delta: the prompt, the completed tool call and its real result.
///
/// Test flow (two `from_stream_turns` turns):
///   Sub-turn 1: LLM → tool_call("tc1", "some_tool", …) + FinalResponse
///               Rig dispatches the tool and records its real result.
///   Sub-turn 2: LLM → error("network failure") → CompletionError.
///
/// The run fails, so the host persists the delta rig never saved.
struct SimpleEchoTool;

impl rig::tool::Tool for SimpleEchoTool {
    const NAME: &'static str = "some_tool";
    type Error = std::convert::Infallible;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "Simple echo tool for testing".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {"x": {"type": "number"}}})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        Ok("real_tool_output".to_string())
    }
}

#[tokio::test]
async fn hard_error_mid_tool_loop_preserves_real_tool_results() -> Result<()> {
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-mid-tool-loop-error";
    let mut memory_state = make_memory_state(&temp_dir);

    // Turn 1: LLM emits tool_call + FinalResponse
    // Turn 2: LLM errors (simulates CompletionError after tool result is in history)
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "some_tool", serde_json::json!({"x": 1})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![MockStreamEvent::error("network failure after tool")],
    ]);

    let shared_model = super::test_utils::shared_model_handle(model);

    let closure_registry = crate::tools::closure::ClosureRegistry::default();
    let mcp_registry = crate::tools::handler::McpToolRegistry::empty();
    let tool_server_handle = rig::tool::server::ToolServer::new()
        .tool(SimpleEchoTool)
        .run();

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(closure_registry),
            mcp_registry: Arc::new(mcp_registry),
            tool_server_handle,
            visible_tool_definitions: vec![rig::completion::ToolDefinition {
                annotations: crate::types::ToolAnnotations::default(),
                name: crate::types::ToolName::new("some_tool")?,
                description: "Simple echo tool for testing".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {"x": {"type": "number"}}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            last_total_tokens: default_last_total_tokens(),
            bus: crate::bus::create_bus(),
            render_registry: ToolRenderRegistry::default(),
            cwd: std::path::PathBuf::from("/tmp"),
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    let result = executor
        .execute(
            ExecuteInput {
                prompt: "do the thing".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver,
            Some(session_id),
        )
        .await;

    // The error must propagate (CompletionError from turn 2, or UnknownToolCall from turn 1)
    assert!(
        result.is_err(),
        "error on sub-call must propagate as Err; got ok"
    );

    // The failed turn persists its delta: the prompt, the completed tool call
    // and its real result.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        3,
        "a mid-tool-loop failure persists the completed tool work; got {persisted:?}"
    );
    assert_eq!(message_text(&persisted[0]).as_deref(), Some("do the thing"));
    assert!(
        matches!(&persisted[1], crate::types::Message::Assistant(_)),
        "the completed tool call is persisted; got {persisted:?}"
    );
    assert!(
        matches!(&persisted[2], crate::types::Message::User { .. }),
        "the real tool result is persisted; got {persisted:?}"
    );

    Ok(())
}
