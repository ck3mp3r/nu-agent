//! rig-memory persistence tests for a turn.
//!
//! The agent keeps its conversation in the rig-memory store: rig loads it
//! before the run and saves it once the run succeeds. These tests drive
//! `execute_turn` with rig's `MockCompletionModel` and read the store back.

use super::*;
use crate::conversation::turn::executor::{ExecuteInput, TurnExecutor, TurnOutcome};
use crate::types::{AssistantContent, AssistantMessage, CallId, ToolCall, ToolFunction, ToolName};
use rig_memory::{Repair, Session, Store as _};

/// Build a turn context over `memory` for `session_id`.
fn context<'a>(
    memory: crate::conversation::state::memory::MemoryOf,
    session_id: &str,
    model: MockCompletionModel,
    config: &'a Config,
) -> TurnContext<'a> {
    TurnContext::new(
        TurnConversation {
            memory,
            conversation_id: session_id.to_string(),
            has_session: true,
            dyn_model: shared_handle(model),
            compaction: test_compaction_config(crate::bus::create_bus()),
        },
        TurnInput {
            prompt: "Hello".to_string(),
            preamble: None,
            max_turns: None,
        },
        tool_infra(
            crate::bus::create_bus(),
            vec![],
            rig::tool::server::ToolServer::new().run(),
        ),
        config,
    )
}

/// A `ToolInfra` with the given bus, visible definitions and tool server.
fn tool_infra(
    bus: crate::bus::Bus,
    visible_tool_definitions: Vec<ToolDefinition>,
    tool_server_handle: rig::tool::server::ToolServerHandle,
) -> executor::ToolInfra {
    executor::ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle,
        visible_tool_definitions,
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        output_repetition: default_output_repetition(),
        repetition_guard: default_repetition_guard(),
        last_total_tokens: default_last_total_tokens(),
        bus,
        render_registry: ToolRenderRegistry::default(),
        cwd: std::path::PathBuf::from("/tmp"),
    }
}

/// An assistant message calling `call_id`, with no result anywhere.
fn dangling_tool_call(call_id: &str) -> Result<crate::types::Message> {
    let name = ToolName::new("read_file").map_err(|_| "non-empty tool name")?;
    Ok(crate::types::Message::Assistant(AssistantMessage::new(
        vec![AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire(call_id),
            ToolFunction::new(name, serde_json::json!({})),
        ))],
    )))
}

/// A user message naming a blob the store does not hold.
fn lost_image_message() -> crate::types::Message {
    crate::types::Message::User {
        content: vec![rig::completion::message::UserContent::image_url(
            "blob:missing.png",
            Some(rig::completion::message::ImageMediaType::PNG),
            None,
        )],
    }
}

/// A completed turn is saved in the rig-memory store in rig-session/4 format,
/// with a header record and change records. rig 0.45 writes no TurnRecord
/// aside: `rig.turn` is a host-appended entry, not a save-path record.
#[tokio::test]
async fn agent_persists_the_conversation_in_the_rig_store() -> Result<()> {
    // -- Setup & Fixtures
    let (_temp_dir, memory) = make_compacting_memory();
    let session_id = "test-rig-persist";
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("Hello from the model!".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let config = Config::default();
    let ctx = context(memory.clone(), session_id, model, &config);

    // -- Exec
    execute_turn(ctx, MockResolver(PermissionDecision::Allow))
        .await
        .map_err(|e| format!("execute_turn should succeed: {e:?}"))?;

    // -- Check: the store holds the prompt and the reply
    let store = memory.rig_store(session_id)?;
    let conversation = rig_memory::load(&*store, session_id)?;
    assert_eq!(
        conversation.messages().len(),
        2,
        "the run's prompt and reply are saved"
    );

    // -- Check: the log holds the header and a change record. rig 0.45 writes
    // no TurnRecord aside: `rig.turn` is a host-appended entry
    // (`HookContext::append_entry`), not a rig save-path record.
    let log = store.read_log(session_id)?;
    let log = String::from_utf8_lossy(&log);
    assert!(
        log.contains(r#""type":"header""#),
        "the log starts with a header record: {log}"
    );
    assert!(
        log.contains(rig_memory::FORMAT),
        "the header names the format: {log}"
    );
    assert!(
        log.contains(&format!(r#""log":"{session_id}""#)),
        "the header names the log: {log}"
    );
    assert!(
        log.contains(r#""type":"push""#),
        "the log holds a change record: {log}"
    );
    Ok(())
}

/// A second turn reloads the conversation the first turn saved.
#[tokio::test]
async fn second_turn_reloads_the_saved_conversation() -> Result<()> {
    // -- Setup & Fixtures
    let (_temp_dir, memory) = make_compacting_memory();
    let session_id = "test-rig-reload";
    let config = Config::default();

    // -- Exec: the first turn
    let first = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("first reply".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    execute_turn(
        context(memory.clone(), session_id, first, &config),
        MockResolver(PermissionDecision::Allow),
    )
    .await
    .map_err(|e| format!("first turn should succeed: {e:?}"))?;

    // -- Exec: the second turn, whose request must carry the first turn
    let second = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("second reply".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let spy = second.clone();
    execute_turn(
        context(memory.clone(), session_id, second, &config),
        MockResolver(PermissionDecision::Allow),
    )
    .await
    .map_err(|e| format!("second turn should succeed: {e:?}"))?;

    // -- Check: the second request starts from the saved conversation plus
    // the new prompt
    let request = spy
        .requests()
        .first()
        .ok_or("the model was called")?
        .clone();
    assert_eq!(
        request.chat_history.len(),
        3,
        "the second run starts from the two saved messages and the new prompt"
    );

    // -- Check: the store now holds both turns
    let store = memory.rig_store(session_id)?;
    let conversation = rig_memory::load(&*store, session_id)?;
    assert_eq!(conversation.messages().len(), 4);
    Ok(())
}

/// A turn round-trips through a file-backed SQLite store and a DirStore.
#[tokio::test]
async fn turn_round_trips_on_each_backend() -> Result<()> {
    for store_type in [
        crate::session::StoreType::Sqlite,
        crate::session::StoreType::Jsonl,
    ] {
        // -- Setup & Fixtures: a store on disk, not in memory
        let temp_dir = tempfile::tempdir()?;
        let rig = crate::session::RigStoreConfig::new(store_type)
            .with_cache_dir(temp_dir.path().join("cache"));
        let memory = Arc::new(crate::session::CachedMemory::new(rig));
        let session_id = "test-round-trip";
        let config = Config::default();

        // -- Exec: two turns
        for reply in ["first reply", "second reply"] {
            let model = MockCompletionModel::from_stream_turns([[
                MockStreamEvent::Text(reply.to_string()),
                MockStreamEvent::final_response_with_default_usage(),
            ]]);
            execute_turn(
                context(memory.clone(), session_id, model, &config),
                MockResolver(PermissionDecision::Allow),
            )
            .await
            .map_err(|e| format!("{store_type:?} turn should succeed: {e:?}"))?;
        }

        // -- Check: the store on disk holds both turns
        let store = memory.rig_store(session_id)?;
        let conversation = rig_memory::load(&*store, session_id)?;
        assert_eq!(
            conversation.messages().len(),
            4,
            "{store_type:?} keeps both turns"
        );
    }
    Ok(())
}

/// A cancelled turn appends its delta to the rig log: the prompt, the committed
/// tool call and its result.
#[tokio::test]
async fn cancelled_turn_appends_its_delta_to_the_rig_log() -> Result<()> {
    // -- Setup & Fixtures: a history with a dangling ToolCall
    let mut memory_state = crate::conversation::state::memory::MemoryState::new(
        crate::session::RigStoreConfig::in_memory(),
    );
    let session_id = "test-cancel-seeded";
    let raw = vec![
        crate::types::Message::user("q1"),
        crate::types::Message::assistant("a1"),
        crate::types::Message::user("q2"),
        dangling_tool_call("tc-dangling")?,
    ];

    // -- Setup & Fixtures: seed the rig store, then capture its log
    {
        let store = memory_state.inner_memory().rig_store(session_id)?;
        let mut conversation = rig_memory::load(&*store, session_id)?;
        conversation.extend(raw);
        rig_memory::save(&*store, session_id, &mut conversation)?;
    }
    let log_before = memory_state
        .inner_memory()
        .rig_store(session_id)?
        .read_log(session_id)?;
    assert!(!log_before.is_empty(), "the seed writes the log");

    // -- Setup & Fixtures: a turn the hook cancels from inside a tool call
    let bus = crate::bus::create_bus();
    let handle = rig::tool::server::ToolServer::new()
        .tool(executor::test_utils::CancellingTool::new(bus.clone()))
        .run();
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
    let config = Config::default();
    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        tool_infra(
            bus.clone(),
            vec![ToolDefinition {
                annotations: crate::types::ToolAnnotations::default(),
                name: ToolName::new("test_cancel_tool")?,
                description: "cancels the turn".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            handle,
        ),
        shared_handle(model),
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
    let outcome = executor
        .execute(
            ExecuteInput {
                prompt: "Hello".to_string(),
                preamble: None,
                span: nu_protocol::Span::test_data(),
            },
            MockResolver(PermissionDecision::Allow),
            Some(session_id),
        )
        .await
        .map_err(|e| format!("cancelled turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::EarlyReturn(_)),
        "a cancelled turn returns EarlyReturn"
    );

    // -- Check: the log grew and the store holds the seeded history plus the
    // turn's delta
    let log_after = memory_state
        .inner_memory()
        .rig_store(session_id)?
        .read_log(session_id)?;
    assert_ne!(
        log_before, log_after,
        "a cancelled turn appends its delta to the rig log"
    );
    let store = memory_state.inner_memory().rig_store(session_id)?;
    let conversation = rig_memory::load(&*store, session_id)?;
    let messages: Vec<crate::types::Message> = conversation
        .messages()
        .iter()
        .map(|message| (**message).clone())
        .collect();
    assert_eq!(
        messages.len(),
        7,
        "the seeded history plus the cancelled turn's delta; got {messages:?}"
    );
    let crate::types::Message::User { content } = &messages[4] else {
        return Err("the delta must start with a user message".into());
    };
    assert!(
        content.iter().any(|item| matches!(
            item,
            crate::types::UserContent::Text(text) if text.text == "Hello"
        )),
        "the turn's prompt is persisted; got {messages:?}"
    );
    assert!(
        matches!(&messages[5], crate::types::Message::Assistant(_)),
        "the committed tool call is persisted; got {messages:?}"
    );
    assert!(
        matches!(&messages[6], crate::types::Message::User { .. }),
        "the committed tool result is persisted; got {messages:?}"
    );
    Ok(())
}

/// `warn_repairs` reports the repairs the session-open path logs.
#[tokio::test]
async fn warn_repairs_reports_the_repairs_it_logs() -> Result<()> {
    // -- Setup & Fixtures: a store holding a message that names a missing blob
    let memory = Arc::new(crate::session::CachedMemory::new(
        crate::session::RigStoreConfig::in_memory(),
    ));
    let session_id = "test-warn-repairs";
    let store = memory.rig_store(session_id)?;
    let mut conversation = rig_memory::load(&*store, session_id)?;
    conversation.extend([lost_image_message()]);
    rig_memory::save(&*store, session_id, &mut conversation)?;

    // -- Exec
    let repairs = memory.warn_repairs(session_id);

    // -- Check: the loss is reported, which is what the path logs
    assert!(
        repairs
            .iter()
            .any(|repair| matches!(repair, Repair::LostImage { .. })),
        "a lost image is reported: {repairs:?}"
    );
    Ok(())
}

/// A log whose message names a blob that is gone reports the loss.
#[test]
fn lost_image_is_reported_as_a_repair() -> Result<()> {
    // -- Setup & Fixtures: a message naming a blob the store does not hold
    let store = rig_memory::MemoryStore::default();
    let mut conversation = rig_memory::load(&store, "test-lost-image")?;
    conversation.extend([lost_image_message()]);
    rig_memory::save(&store, "test-lost-image", &mut conversation)?;

    // -- Exec
    let session = Session::read(&store)?;

    // -- Check: the loss is reported, which the session-open path logs
    assert!(
        session
            .repairs
            .iter()
            .any(|repair| matches!(repair, Repair::LostImage { .. })),
        "a lost image is reported: {:?}",
        session.repairs
    );
    Ok(())
}
