//! Shared fixtures for the recover test modules.
//!
//! Holds the executor, store and tool helpers every topical recover test file
//! needs. Sub-files pull them in with `use super::*;`.

use super::*;

/// A `ToolInfra` with no tools and the given bus.
pub(super) fn tool_infra(bus: crate::bus::Bus) -> executor::ToolInfra {
    executor::ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: rig::tool::server::ToolServer::new().run(),
        visible_tool_definitions: vec![],
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        output_repetition: default_output_repetition(),
        repetition_guard: default_repetition_guard(),
        last_total_tokens: default_last_total_tokens(),
        turn_usage: crate::hook::TurnUsage::default(),
        session_usage: crate::hook::TurnUsage::default(),
        bus,
        render_registry: ToolRenderRegistry::default(),
        cwd: std::path::PathBuf::from("/tmp"),
    }
}

/// A `TurnExecutor` over `memory_state` with a deterministic mock model.
pub(super) fn executor<'a>(
    config: &'a Config,
    memory_state: &'a mut MemoryState,
) -> TurnExecutor<'a, MemoryState> {
    TurnExecutor::new(
        config,
        memory_state,
        tool_infra(crate::bus::create_bus()),
        shared_handle(MockCompletionModel::text("unused")),
        test_compaction_config(crate::bus::create_bus()),
    )
}

/// A `TurnExecutor` over `memory_state` with a deterministic mock model and the
/// given bus, so a test can observe the events the executor publishes.
pub(super) fn executor_with_bus<'a>(
    config: &'a Config,
    memory_state: &'a mut MemoryState,
    bus: crate::bus::Bus,
) -> TurnExecutor<'a, MemoryState> {
    TurnExecutor::new(
        config,
        memory_state,
        tool_infra(bus.clone()),
        shared_handle(MockCompletionModel::text("unused")),
        test_compaction_config(bus),
    )
}

/// An assistant message calling each id in `call_ids`, with no result anywhere.
pub(super) fn assistant_tool_calls(call_ids: &[&str]) -> Result<Message> {
    let name = ToolName::new("read_file").map_err(|_| "non-empty tool name")?;
    let content = call_ids
        .iter()
        .map(|call_id| {
            AssistantContent::ToolCall(ToolCall::new(
                CallId::from_wire(*call_id),
                ToolFunction::new(name.clone(), serde_json::json!({})),
            ))
        })
        .collect();
    Ok(Message::Assistant(AssistantMessage::new(content)))
}

/// Seed the session's rig store with `messages`.
pub(super) fn seed(
    memory_state: &MemoryState,
    session_id: &str,
    messages: Vec<Message>,
) -> Result<()> {
    let store = memory_state.inner_memory().rig_store(session_id)?;
    let mut conversation = rig_memory::load(&*store, session_id)?;
    conversation.extend(messages);
    rig_memory::save(&*store, session_id, &mut conversation)?;
    Ok(())
}

/// The conversation the session's rig store holds.
pub(super) fn stored(memory_state: &MemoryState, session_id: &str) -> Result<Vec<Message>> {
    let store = memory_state.inner_memory().rig_store(session_id)?;
    let conversation = rig_memory::load(&*store, session_id)?;
    Ok(conversation
        .messages()
        .iter()
        .map(|message| (**message).clone())
        .collect())
}

/// A `TurnContext` whose tool server exposes `tool` as `name`.
pub(super) fn with_tool<'a, T>(
    ctx: TurnContext<'a>,
    config: &'a Config,
    bus: crate::bus::Bus,
    tool: T,
    name: &str,
) -> Result<TurnContext<'a>>
where
    T: rig::tool::Tool + 'static,
{
    let handle = rig::tool::server::ToolServer::new().tool(tool).run();
    let tool_infra = executor::ToolInfra {
        closure_registry: Arc::new(ClosureRegistry::default()),
        mcp_registry: Arc::new(McpToolRegistry::empty()),
        tool_server_handle: handle,
        visible_tool_definitions: vec![ToolDefinition {
            annotations: crate::types::ToolAnnotations::default(),
            name: ToolName::new(name)?,
            description: "a test tool".to_string(),
            parameters: serde_json::json!({"type": "object", "properties": {}}),
        }],
        circuit_breaker: default_circuit_breaker(),
        doom_state: default_doom_state(),
        output_repetition: default_output_repetition(),
        repetition_guard: default_repetition_guard(),
        last_total_tokens: default_last_total_tokens(),
        turn_usage: crate::hook::TurnUsage::default(),
        session_usage: crate::hook::TurnUsage::default(),
        bus,
        render_registry: ToolRenderRegistry::default(),
        cwd: std::path::PathBuf::from("/tmp"),
    };
    Ok(TurnContext::new(
        ctx.conversation,
        ctx.input,
        tool_infra,
        config,
    ))
}
