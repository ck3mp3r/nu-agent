//!
//! The settle transaction: one turn's row, its entry keys and its call rows
//! land together, and a turn that settles nothing writes nothing.

use super::*;
use crate::conversation::state::memory::MemoryState;
use crate::conversation::turn::executor::{ExecuteInput, TurnExecutor, TurnOutcome};
use agnt_runtime::session::StoreType;
use agnt_storage::{Entries, Storage, ToolCallStatus, ToolCalls, Turns};

/// The tool the settled turn calls.
const ECHO_TOOL: &str = "test_settle_tool";

/// A tool that answers its arguments: the turn settles a real success.
struct EchoTool;

impl rig::tool::Tool for EchoTool {
    const NAME: &'static str = ECHO_TOOL;
    type Error = std::convert::Infallible;
    type Args = serde_json::Value;
    type Output = String;

    fn description(&self) -> String {
        "echoes its arguments".to_string()
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

/// Build a turn context over `memory` for `session_id`.
fn context<'a>(
    memory: crate::conversation::state::memory::MemoryOf,
    session_id: &str,
    model: MockCompletionModel,
    config: &'a Config,
    has_session: bool,
) -> Result<TurnContext<'a>> {
    Ok(TurnContext::new(
        TurnConversation {
            memory,
            conversation_id: session_id.to_string(),
            agent: "tester".to_string(),
            has_session,
            dyn_model: shared_handle(model),
            compaction: test_compaction_config(crate::bus::create_bus()),
        },
        TurnInput {
            prompt: "Hello".to_string(),
            preamble: None,
            max_turns: None,
        },
        executor::ToolInfra {
            store_latch: Default::default(),
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle: rig::tool::server::ToolServer::new().tool(EchoTool).run(),
            visible_tool_definitions: vec![ToolDefinition {
                annotations: crate::types::ToolAnnotations::default(),
                name: ToolName::new(ECHO_TOOL)?,
                description: "echoes its arguments".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            last_total_tokens: default_last_total_tokens(),
            turn_usage: crate::hook::TurnUsage::default(),
            session_usage: crate::hook::TurnUsage::default(),
            bus: crate::bus::create_bus(),
            cwd: std::path::PathBuf::from("/tmp"),
        },
        config,
    ))
}

/// The fact tables over the database `rig` opens.
fn facts(rig: &agnt_runtime::session::RigStoreConfig) -> Result<Arc<Storage>> {
    Ok(rig.facts_blocking()?)
}

/// A settled turn records its row, the keys of the entries it persisted and the
/// status its call settled with, in the one unit.
#[tokio::test]
async fn settled_turn_records_its_row_its_entry_keys_and_its_call() -> Result<()> {
    // -- Setup & Fixtures: a file store, so the facts outlive the turn
    let temp_dir = tempfile::tempdir()?;
    let rig = agnt_runtime::session::RigStoreConfig::new(StoreType::Sqlite)
        .with_cache_dir(temp_dir.path().join("cache"));
    let memory = Arc::new(agnt_runtime::session::CachedMemory::new(rig.clone()));
    let session_id = "test-settle";
    let config = Config::default();
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", ECHO_TOOL, serde_json::json!({"x": 1})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("done".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    // -- Exec
    execute_turn(
        context(memory, session_id, model, &config, true)?,
        MockResolver(PermissionDecision::Allow),
    )
    .await
    .map_err(|e| format!("turn should succeed: {e:?}"))?;

    // -- Check
    let storage = facts(&rig)?;
    let turns = Turns::new(Arc::clone(&storage)).read(session_id).await?;
    assert_eq!(turns.len(), 1, "one turn, one row: {turns:?}");
    assert_eq!(turns[0].turn_seq, 1);
    assert_eq!(
        turns[0].model,
        format!("{}/{}", config.provider, config.model)
    );
    assert_eq!(turns[0].agent, "tester");
    assert!(turns[0].submitted_millis <= turns[0].settled_millis);

    let entries = Entries::new(Arc::clone(&storage)).read(session_id).await?;
    assert!(!entries.is_empty(), "the turn persisted entries");
    assert!(
        entries.iter().all(|entry| entry.turn_seq == 1),
        "every entry of the turn points at it: {entries:?}"
    );

    let calls = ToolCalls::new(storage).read(session_id).await?;
    assert_eq!(calls.len(), 1, "one call, one row: {calls:?}");
    assert_eq!(calls[0].call, "tc1");
    assert_eq!(calls[0].turn_seq, 1);
    assert_eq!(calls[0].status, ToolCallStatus::Success);
    Ok(())
}

/// A cancelled turn settles nothing: the recovery that persists its prompt
/// records the turn later, never the cancelled settle.
#[tokio::test]
async fn cancelled_turn_records_no_facts() -> Result<()> {
    // -- Setup & Fixtures: a tool that cancels the turn from inside the call
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = MemoryState::new(
        agnt_runtime::session::RigStoreConfig::new(StoreType::Sqlite)
            .with_cache_dir(temp_dir.path().join("cache")),
    );
    let session_id = "test-settle-cancel";
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
        executor::ToolInfra {
            store_latch: Default::default(),
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle: handle,
            visible_tool_definitions: vec![ToolDefinition {
                annotations: crate::types::ToolAnnotations::default(),
                name: ToolName::new("test_cancel_tool")?,
                description: "cancels the turn".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            last_total_tokens: default_last_total_tokens(),
            turn_usage: crate::hook::TurnUsage::default(),
            session_usage: crate::hook::TurnUsage::default(),
            bus: bus.clone(),
            cwd: std::path::PathBuf::from("/tmp"),
        },
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
    assert!(matches!(outcome, TurnOutcome::EarlyReturn(_)));

    // -- Check: the cancelled settle wrote nothing; the recovery closed one
    // turn, holding the call that ran and the keys of the delta it saved.
    let facts = memory_state.inner_memory().facts().await?;
    let read = facts.read(session_id).await?;
    assert!(
        !read.entries.is_empty(),
        "the recovery attributes the entries it saved: {:?}",
        read.entries
    );
    assert!(
        read.entries.iter().all(|entry| entry.turn_seq == 1),
        "every key points at the closed turn: {:?}",
        read.entries
    );
    assert_eq!(read.calls.len(), 1, "one call, one row: {:?}", read.calls);
    assert_eq!(read.calls[0].call, "tc1");
    assert_eq!(read.calls[0].turn_seq, 1);
    Ok(())
}

/// A session-less turn records nothing: no log is saved, so no fact is written.
#[tokio::test]
async fn session_less_turn_records_no_facts() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = tempfile::tempdir()?;
    let rig = agnt_runtime::session::RigStoreConfig::new(StoreType::Sqlite)
        .with_cache_dir(temp_dir.path().join("cache"));
    let memory = Arc::new(agnt_runtime::session::CachedMemory::new(rig.clone()));
    let session_id = "test-settle-transient";
    let config = Config::default();
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("one shot".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    // -- Exec
    execute_turn(
        context(memory, session_id, model, &config, false)?,
        MockResolver(PermissionDecision::Allow),
    )
    .await
    .map_err(|e| format!("turn should succeed: {e:?}"))?;

    // -- Check
    let storage = facts(&rig)?;
    assert!(
        Turns::new(Arc::clone(&storage))
            .read(session_id)
            .await?
            .is_empty(),
        "no session, no turn row"
    );
    assert!(
        Entries::new(Arc::clone(&storage))
            .read(session_id)
            .await?
            .is_empty(),
        "no session, no entry keys"
    );
    assert!(
        ToolCalls::new(storage).read(session_id).await?.is_empty(),
        "no session, no call rows"
    );
    Ok(())
}
