//! The dispatch gate's guard arms: a skipped call opens a started row, and
//! rig's own skipped outcome settles it.
//!
//! One case per guard arm — the sub-turn cap, the doom loop, the circuit
//! breaker. Each drives a real rig stream, so the arms and the settle run as
//! they do in production: the gate publishes `ToolStarted` before any arm runs,
//! rig answers the model with the skip reason, and `on_outcome` publishes the
//! single `ToolCompleted` that closes the row with the did-not-run status and
//! the refusal as its result text.

use super::*;

use crate::tools::mcp::client::McpToolDefinition;

/// Resolver that allows every call: these tests drive the guard arms, not the
/// permission flow.
#[derive(Clone)]
struct AllowAllResolver;

impl crate::hook::permission_resolver::AsyncPermissionResolver for AllowAllResolver {
    async fn resolve(
        &self,
        _tool_name: &str,
        _arguments: &str,
        _tool_call_id: Option<String>,
        _stream: &crate::bus::Stream,
        _facts: &crate::bus::FactsTx,
        _cancel: &crate::bus::CancelTx,
    ) -> crate::hook::permission_resolver::PermissionDecision {
        crate::hook::permission_resolver::PermissionDecision::Allow
    }
}

/// The sub-turn cap blocks the response's second call. The blocked call still
/// publishes its started record — carrying the call's own data — and rig's
/// skipped outcome settles it once, cancelled, with the cap's refusal as the
/// result text the model saw.
#[tokio::test]
async fn on_dispatch_subturn_cap_publishes_started_and_settles_cancelled() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let fixture = HookStateFixture::default().with_compaction_bus(bus.clone());
    let hook = build_chain_with_gates(
        bus.clone(),
        AllowAllResolver,
        fixture,
        Some(1),
        Arc::new(crate::tools::handler::McpToolRegistry::empty()),
    );
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "add", serde_json::json!({"x": 1, "y": 2})),
            MockStreamEvent::tool_call("tc2", "read", serde_json::json!({"path": "/tmp/f"})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![MockStreamEvent::final_response_with_default_usage()],
    ])
    .erase();
    let agent = rig::agent::AgentBuilder::new(model)
        .add_hook(hook)
        .tool(rig::test_utils::MockAddTool)
        .dynamic_tool(stub_tool("read")?)
        .build();
    // Subscribe BEFORE driving the stream — broadcast sends are not buffered
    // for later subscribers.
    let mut ui_rx = bus.stream().subscribe();

    // -- Exec: the cap admits the first call and skips the second.
    let mut stream = agent.prompt("run").max_turns(16).stream();
    while stream.next().await.is_some() {}

    // -- Check: the blocked call is opened and settled exactly once.
    let observed = observed(&mut ui_rx);
    assert_eq!(
        observed
            .iter()
            .map(|record| (record.name.as_str(), record.started))
            .collect::<Vec<_>>(),
        vec![
            ("add", true),
            ("add", false),
            ("read", true),
            ("read", false)
        ],
        "the blocked call must publish its started record and exactly one completion"
    );
    let blocked = observed.last().ok_or("the blocked call must be settled")?;
    assert_eq!(
        blocked.status,
        Some(ToolStatus::Cancelled),
        "a guard skip settles cancelled"
    );
    let reason = blocked
        .text
        .as_deref()
        .ok_or("the blocked call must carry the refusal as its result text")?;
    assert!(
        reason.starts_with("Sub-turn tool call limit reached (1)"),
        "the result text must be the refusal rig answered the model with, not derived: {reason}"
    );
    Ok(())
}

/// The doom loop blocks the call on its first detection in the turn attempt.
/// The blocked call still publishes its started record, and rig's skipped
/// outcome settles it once with the steering message as the result text.
#[tokio::test]
async fn on_dispatch_doom_loop_publishes_started_and_settles_cancelled() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    // Four identical signatures are already recorded: this call is the fifth.
    let fixture = HookStateFixture::default()
        .with_compaction_bus(bus.clone())
        .with_repetition_guard(true)
        .with_doom_signatures("add", "{}", 4);
    let hook = build_chain_with_gates(
        bus.clone(),
        AllowAllResolver,
        fixture,
        None,
        Arc::new(crate::tools::handler::McpToolRegistry::empty()),
    );
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "add", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![MockStreamEvent::final_response_with_default_usage()],
    ])
    .erase();
    let agent = rig::agent::AgentBuilder::new(model)
        .add_hook(hook)
        .tool(rig::test_utils::MockAddTool)
        .build();
    let mut ui_rx = bus.stream().subscribe();

    // -- Exec
    let mut stream = agent.prompt("run").max_turns(16).stream();
    while stream.next().await.is_some() {}

    // -- Check
    let observed = observed(&mut ui_rx);
    assert_eq!(
        observed
            .iter()
            .map(|record| (record.name.as_str(), record.started))
            .collect::<Vec<_>>(),
        vec![("add", true), ("add", false)],
        "the skipped call must publish its started record and exactly one completion"
    );
    assert_eq!(
        observed[1].status,
        Some(ToolStatus::Cancelled),
        "a guard skip settles cancelled"
    );
    let reason = observed[1]
        .text
        .as_deref()
        .ok_or("the skipped call must carry the refusal as its result text")?;
    assert!(
        reason.starts_with("Doom loop detected: 'add'"),
        "the result text must be the steering message rig answered the model with: {reason}"
    );
    Ok(())
}

/// The doom loop's fourth detection stops the run instead of steering it. The
/// stopped call publishes its started record and the gate pairs it with the
/// not-run completion: a stop fails the dispatch before `on_outcome` fires, so
/// nothing else would settle a row that would otherwise spin forever.
#[tokio::test]
async fn on_dispatch_doom_loop_stop_pairs_the_started_record() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let fixture = HookStateFixture::default()
        .with_compaction_bus(bus.clone())
        .with_repetition_guard(true)
        .with_doom_signatures("add", "{}", 4)
        .with_doom_escalation(4);
    let hook = build_chain_with_gates(
        bus.clone(),
        AllowAllResolver,
        fixture,
        None,
        Arc::new(crate::tools::handler::McpToolRegistry::empty()),
    );
    let model = MockCompletionModel::from_stream_turns([vec![
        MockStreamEvent::tool_call("tc1", "add", serde_json::json!({})),
        MockStreamEvent::final_response_with_default_usage(),
    ]])
    .erase();
    let agent = rig::agent::AgentBuilder::new(model)
        .add_hook(hook)
        .tool(rig::test_utils::MockAddTool)
        .build();
    let mut ui_rx = bus.stream().subscribe();

    // -- Exec
    let mut items = Vec::new();
    let mut stream = agent.prompt("run").max_turns(16).stream();
    while let Some(item) = stream.next().await {
        items.push(item);
    }

    // -- Check: one started record, one not-run completion, and the stop.
    let observed = observed(&mut ui_rx);
    assert_eq!(
        observed
            .iter()
            .map(|record| (record.name.as_str(), record.started))
            .collect::<Vec<_>>(),
        vec![("add", true), ("add", false)],
        "the stopped call must publish its started record and the completion that settles it"
    );
    assert_eq!(
        observed[1].status,
        Some(ToolStatus::Cancelled),
        "a stopped call settles cancelled"
    );
    assert_eq!(
        observed[1].message.as_deref(),
        Some(CANCELLED_DISPATCH_MESSAGE),
        "the completion carries the did-not-run message"
    );
    let last = items
        .pop()
        .ok_or("the stream must yield at least one item")?;
    let error = last.err().ok_or("the run must end with the doom stop")?;
    let rig::completion::PromptError::Cancelled { reason, .. } = error else {
        return Err("the run must end with Cancelled".into());
    };
    assert!(
        reason.starts_with("Doom loop stopped:"),
        "the stop reason names the doom loop: {reason}"
    );
    Ok(())
}

/// The circuit breaker blocks every call of a disabled MCP server. The blocked
/// call still publishes its started record, and rig's skipped outcome settles
/// it once with the disabled-server message as the result text.
#[tokio::test]
async fn on_dispatch_circuit_breaker_publishes_started_and_settles_cancelled() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let mcp_registry = McpToolRegistry::from_tools([McpToolDefinition {
        server: "srv".to_string(),
        name: "mcp__srv__ping".to_string(),
        raw_name: "ping".to_string(),
        description: None,
        parameters: None,
    }])
    .map_err(|error| format!("should build the MCP registry: {error}"))?;
    mcp_registry
        .set_server_enabled("srv", false)
        .map_err(|error| format!("should disable the server: {error}"))?;
    let fixture = HookStateFixture::default().with_compaction_bus(bus.clone());
    let hook = build_chain_with_gates(
        bus.clone(),
        AllowAllResolver,
        fixture,
        None,
        Arc::new(mcp_registry),
    );
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "mcp__srv__ping", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![MockStreamEvent::final_response_with_default_usage()],
    ])
    .erase();
    let agent = rig::agent::AgentBuilder::new(model)
        .add_hook(hook)
        .dynamic_tool(stub_tool("mcp__srv__ping")?)
        .build();
    let mut ui_rx = bus.stream().subscribe();

    // -- Exec
    let mut stream = agent.prompt("run").max_turns(16).stream();
    while stream.next().await.is_some() {}

    // -- Check
    let observed = observed(&mut ui_rx);
    assert_eq!(
        observed
            .iter()
            .map(|record| (record.name.as_str(), record.started))
            .collect::<Vec<_>>(),
        vec![("mcp__srv__ping", true), ("mcp__srv__ping", false)],
        "the skipped call must publish its started record and exactly one completion"
    );
    assert_eq!(
        observed[1].status,
        Some(ToolStatus::Cancelled),
        "a guard skip settles cancelled"
    );
    let reason = observed[1]
        .text
        .as_deref()
        .ok_or("the skipped call must carry the refusal as its result text")?;
    assert!(
        reason.starts_with("MCP server 'srv' is disabled"),
        "the result text must be the disabled-server message rig answered the model with: {reason}"
    );
    Ok(())
}

// -- Test Support

/// A dynamic tool registered under `name`, so rig accepts a streamed call to
/// it. The body answers "contents": a call a gate blocks never reaches it.
fn stub_tool(name: &str) -> Result<rig::tool::DynamicTool> {
    Ok(rig::tool::DynamicTool::new(
        ToolName::new(name)?,
        "A stub the gate never runs",
        serde_json::json!({"type": "object", "properties": {}}),
        |_args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("contents")) }),
    ))
}

/// One tool record on the stream, as a surface reads it: which call it names,
/// whether it opens or closes the call, and — for a completion — the status and
/// the result text, kept only when it reports the call did not run.
#[derive(Debug, PartialEq, Eq)]
struct Observed {
    name: String,
    started: bool,
    status: Option<ToolStatus>,
    text: Option<String>,
    message: Option<String>,
}

/// Drain the stream's tool records, in publish order.
fn observed(rx: &mut crate::bus::Subscription<SessionRecord>) -> Vec<Observed> {
    let mut observed = Vec::new();
    while let Ok(event) = rx.try_recv() {
        match event {
            SessionRecord::Tool(ToolRecord::Started { call }) => observed.push(Observed {
                name: call.name.0.clone(),
                started: true,
                status: None,
                text: None,
                message: None,
            }),
            SessionRecord::Tool(ToolRecord::Completed { call, outcome }) => {
                observed.push(Observed {
                    name: call.name.0.clone(),
                    started: false,
                    status: Some(outcome.status),
                    text: (outcome.status == ToolStatus::Cancelled).then(|| outcome.text.clone()),
                    message: outcome.message.clone(),
                })
            }
            _ => (),
        }
    }
    observed
}
