//! The two-run turn: the side-call's summary is spliced between the runs, and the
//! second run answers the turn.

use super::*;

use rig_memory::summary_author;

const CONV: &str = "conv-1";
const SUMMARY: &str = "the older turns, summarized";
const ANSWER: &str = "the answer to the turn";
const FINAL: &str = "pass one's real answer";

// region:    --- Test Support

/// The spec the turn's model runs with: a 60k window, so a conversation of five
/// 40k-character turns is over its input limit less the reserve.
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

/// WHEN the threshold decision asks for the summary on its own, THE execute_turn
/// loop SHALL splice it between the two runs AND SHALL answer with pass two's
/// text.
#[tokio::test]
async fn side_call_summary_splices_between_the_runs_and_pass_two_answers() -> Result<()> {
    // -- Setup & Fixtures: an over-threshold session, and a model whose first
    // scripted turn answers the side-call, whose second answers pass one, and
    // whose third answers the turn over the spliced summary.
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
            MockStreamEvent::Text(FINAL.to_string()),
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

    // -- Check: pass two answered, and the folded store leads with the summary.
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
        "the side-call must not read back as an assistant turn"
    );
    assert!(
        assistant_texts(&conversation)
            .iter()
            .any(|text| text == FINAL),
        "pass one's own answer must stay in the history, assistant texts: {:?}",
        assistant_texts(&conversation)
    );
    Ok(())
}
