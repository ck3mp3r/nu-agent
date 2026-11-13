//! The reply-consumption loop: the patched call's reply becomes the stored
//! summary between the two runs, and the second run answers the turn.

use super::*;

use agnt_kernel::{CompactionRecord, SessionRecord};
use rig_memory::summary_author;

const CONV: &str = "conv-1";
const SUMMARY: &str = "the older turns, summarized";
const ANSWER: &str = "the answer to the turn";
const FINAL: &str = "pass one's real answer";
const SIDE_CALL_SUMMARY: &str = "the summary the settle site asked for";
const REPLY_TOOL: &str = "test_supersede_reply_tool";

// region:    --- Test Support

/// The tool the patched call calls: its answer carries a tool call and no
/// text, so the reply cannot be a summary and rig continues the run.
struct ReplyTool;

impl rig::tool::Tool for ReplyTool {
    const NAME: &'static str = REPLY_TOOL;
    type Error = std::convert::Infallible;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "answers its arguments".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        args: Self::Args,
    ) -> std::result::Result<Self::Output, Self::Error> {
        Ok(args.to_string())
    }
}

/// The spec the turn's model runs with: a 60k window, so a conversation of
/// five 40k-character turns is over its input limit less the reserve and the
/// kept window still leaves room for the summary.
fn turn_spec() -> Result<rig::catalog::ModelSpec> {
    crate::conversation::compaction::compactor::resolve_model_spec(
        "openai",
        "gpt-4o",
        Some(60_000),
        Some(1_024),
        None,
    )
    .ok_or_else(|| "openai must resolve to a registered provider id".into())
}

/// Seed the session's rig store with a conversation over the turn's limit.
async fn seed_conversation(memory: &crate::conversation::state::memory::MemoryOf) -> Result<()> {
    let store = memory
        .rig_store(CONV)
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let mut conversation = rig_memory::load(&*store, CONV)?;
    conversation.extend(
        (0..5).map(|i| crate::types::Message::user(format!("message {i} {}", "q".repeat(40_000)))),
    );
    rig_memory::save(&*store, CONV, &mut conversation)?;
    Ok(())
}

/// Every user text the stored conversation holds.
fn user_texts(conversation: &rig::conversation::Conversation) -> Vec<String> {
    conversation
        .messages()
        .iter()
        .filter_map(|message| match &**message {
            crate::types::Message::User { content } => Some(
                content
                    .iter()
                    .filter_map(|item| match item {
                        crate::types::UserContent::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

/// Every assistant text the stored conversation holds.
fn assistant_texts(conversation: &rig::conversation::Conversation) -> Vec<String> {
    conversation
        .messages()
        .iter()
        .filter_map(|message| match &**message {
            crate::types::Message::Assistant(turn) => Some(
                turn.content
                    .iter()
                    .filter_map(|item| match item {
                        crate::types::AssistantContent::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

// endregion: --- Test Support

/// WHEN the patched call answers with a summary, THE execute_turn loop SHALL
/// splice it between the two runs, SHALL remove the reply entry from the folded
/// conversation, AND SHALL answer with pass two's text.
#[tokio::test]
async fn summary_reply_splices_between_the_runs_and_pass_two_answers() -> Result<()> {
    // -- Setup & Fixtures: an over-threshold session, and a model whose first
    // turn is the summary and whose second answers the turn.
    let (temp_dir, memory) = make_compacting_memory();
    let _keep_alive = temp_dir;
    seed_conversation(&memory).await?;
    let store = memory
        .rig_store(CONV)
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::Text(SUMMARY.to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text(ANSWER.to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ])
    .priced_as(turn_spec()?);
    let bus = crate::bus::Bus::default();
    let conversation = TurnConversation {
        memory,
        conversation_id: CONV.to_string(),
        agent: String::new(),
        has_session: true,
        dyn_model: shared_handle(model),
        compaction: test_compaction_config(bus.clone()),
    };
    let input = TurnInput {
        prompt: "Hello".to_string(),
        preamble: None,
        max_turns: None,
    };
    let tool_infra = executor::ToolInfra {
        store_latch: Default::default(),
        pending_summary: Default::default(),
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        visible_tool_definitions: vec![],
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        output_repetition: default_output_repetition(),
        repetition_guard: default_repetition_guard(),
        meter: default_meter(),
        turn_usage: crate::hook::TurnUsage::default(),
        session_usage: crate::hook::TurnUsage::default(),
        bus: bus.clone(),
        cwd: std::path::PathBuf::from("/tmp"),
    };
    let config = Config::default();
    let ctx = TurnContext::new(conversation, input, tool_infra, &config);

    // -- Exec
    let result = execute_turn(ctx, MockResolver(PermissionDecision::Allow))
        .await
        .map_err(|e| format!("execute_turn should succeed: {e:?}"))?;

    // -- Check: pass two answered, and the folded store holds the summary.
    assert!(
        result.text.contains(ANSWER),
        "the turn must report pass two's text, got {:?}",
        result.text
    );
    let conversation = rig_memory::load(&*store, CONV)?;
    let first = conversation
        .entries()
        .next()
        .ok_or("the compacted conversation is non-empty")?;
    assert_eq!(
        first.author,
        Some(&summary_author()),
        "the next run must load the summary first"
    );
    assert!(
        user_texts(&conversation)
            .iter()
            .any(|text| text.contains(SUMMARY)),
        "the folded conversation must carry the summary"
    );
    assert!(
        assistant_texts(&conversation)
            .iter()
            .all(|text| text != SUMMARY),
        "the internal call must not read back as an assistant turn"
    );
    Ok(())
}

/// WHEN the patched call answers a tool call with no text, THEN THE settle site
/// SHALL replace the refused reply with a summary asked on its own: no `Failed`
/// row SHALL be published for the refused verdict, the store SHALL lead with the
/// side-call's summary, and pass two SHALL still answer the turn.
#[tokio::test]
async fn textless_reply_is_replaced_by_the_side_call_summary() -> Result<()> {
    // -- Setup & Fixtures: an over-threshold session, and a model whose patched
    // call answers with a tool call and no text; rig dispatches the call and the
    // run ends with a real answer, the settle site asks for the summary on its
    // own, and pass two answers the turn over it.
    let (temp_dir, memory) = make_compacting_memory();
    let _keep_alive = temp_dir;
    seed_conversation(&memory).await?;
    let store = memory
        .rig_store(CONV)
        .await
        .map_err(|e| format!("rig store should open: {e:?}"))?;
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", REPLY_TOOL, serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text(FINAL.to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        // The side-call that replaces the refused reply.
        vec![
            MockStreamEvent::Text(SIDE_CALL_SUMMARY.to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text(ANSWER.to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ])
    .priced_as(turn_spec()?);
    let bus = crate::bus::Bus::default();
    let mut records = bus.stream().subscribe();
    let conversation = TurnConversation {
        memory,
        conversation_id: CONV.to_string(),
        agent: String::new(),
        has_session: true,
        dyn_model: shared_handle(model),
        compaction: test_compaction_config(bus.clone()),
    };
    let input = TurnInput {
        prompt: "Hello".to_string(),
        preamble: None,
        max_turns: None,
    };
    let tool_infra = executor::ToolInfra {
        store_latch: Default::default(),
        pending_summary: Default::default(),
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: rig::tool::server::ToolServer::new().tool(ReplyTool).run(),
        visible_tool_definitions: vec![ToolDefinition {
            annotations: crate::types::ToolAnnotations::default(),
            name: ToolName::new(REPLY_TOOL)?,
            description: "answers its arguments".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }],
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        output_repetition: default_output_repetition(),
        repetition_guard: default_repetition_guard(),
        meter: default_meter(),
        turn_usage: crate::hook::TurnUsage::default(),
        session_usage: crate::hook::TurnUsage::default(),
        bus: bus.clone(),
        cwd: std::path::PathBuf::from("/tmp"),
    };
    let config = Config::default();
    let ctx = TurnContext::new(conversation, input, tool_infra, &config);

    // -- Exec
    let result = execute_turn(ctx, MockResolver(PermissionDecision::Allow))
        .await
        .map_err(|e| format!("execute_turn should succeed: {e:?}"))?;

    // -- Check: the refused verdict published nothing, the side-call's summary
    // leads the store, pass one's real answer stays, and pass two answered the
    // turn.
    let mut failed = 0;
    while let Ok(record) = records.try_recv() {
        if matches!(
            record,
            SessionRecord::Compaction(CompactionRecord::Failed { .. })
        ) {
            failed += 1;
        }
    }
    assert_eq!(
        failed, 0,
        "the refused verdict is replaced by the side-call, so no Failed row is published"
    );
    let conversation = rig_memory::load(&*store, CONV)?;
    let first = conversation
        .entries()
        .next()
        .ok_or("the compacted conversation is non-empty")?;
    assert_eq!(
        first.author,
        Some(&rig_memory::summary_author()),
        "the store must lead with the side-call's summary"
    );
    assert!(
        assistant_texts(&conversation)
            .iter()
            .any(|text| text == SIDE_CALL_SUMMARY),
        "the side-call's answer must be in the store, stored assistant texts: {:?}",
        assistant_texts(&conversation)
    );
    assert!(
        result.text.contains(ANSWER),
        "pass two must still answer the turn, got {:?}",
        result.text
    );
    Ok(())
}
