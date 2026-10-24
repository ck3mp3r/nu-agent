use super::*;

use crate::hook::permission_resolver::{AsyncPermissionResolver, PermissionDecision};
use crate::hook::turn_tools::TurnToolLog;

/// A resolver that allows every tool, so the turn reaches the tool instead of
/// stopping at the permission gate. The accumulator is what these tests
/// exercise, not the policy.
#[derive(Clone)]
struct AllowAllResolver;

impl AsyncPermissionResolver for AllowAllResolver {
    async fn resolve(
        &self,
        _tool_name: &str,
        _arguments: &str,
        _tool_call_id: Option<String>,
        _bus: &Bus,
    ) -> PermissionDecision {
        PermissionDecision::Allow
    }
}

/// A hook chain over a scripted model, with the turn's tool log cloned out
/// before the hook moves into the agent builder — the same order the executor
/// uses.
struct ToolTurn {
    hook: HookChain<AllowAllResolver>,
    turn_tools: Arc<TurnToolLog>,
}

/// Build the hook chain a turn runs with, and clone its tool log out.
fn tool_turn(model: &MockCompletionModel) -> Result<ToolTurn> {
    let bus = Bus::default();
    let memory = Arc::new(CachedMemory::new(
        crate::session::RigStoreConfig::in_memory(),
    ));
    let hook = HookChain::new(
        HookEnv {
            bus: bus.clone(),
            cwd: std::path::PathBuf::from("/tmp"),
        },
        AllowAllResolver,
        Arc::new(crate::tools::closure::ClosureRegistry::default()),
        Arc::new(crate::tools::handler::McpToolRegistry::empty()),
        crate::tools::handler::builtin_tool::ToolRenderRegistry::default(),
        None,
        HookState {
            circuit_breaker: Arc::new(Mutex::new(
                crate::tools::mcp::circuit_breaker::McpCircuitBreaker::default(),
            )),
            doom_state: Arc::new(Mutex::new(DoomLoopState::default())),
            output_repetition: Arc::new(Mutex::new(RepetitionState::default())),
            repetition_guard: false,
            memory,
            conversation_id: "turn-tools-conv".to_string(),
            compaction: CompactionConfig {
                compactor: NuCompactor::new(
                    MockCompletionModel::text("summary").erase(),
                    Bus::default(),
                    rig_memory::CompactionPolicy::default(),
                    test_model_spec(),
                ),
                params: CompactionParams::default(),
                threshold_tokens: None,
                threshold_pct: crate::config::defaults::PROACTIVE_THRESHOLD_PCT,
            },
            model: model.clone().erase(),
            last_total_tokens: Arc::new(Mutex::new(None)),
        },
    );
    let turn_tools = hook.turn_tools();
    Ok(ToolTurn { hook, turn_tools })
}

/// Drive one turn on `hook` against `model`, with the `add` tool registered.
async fn drive_add(hook: HookChain<AllowAllResolver>, model: MockCompletionModel) -> Result<()> {
    let agent = rig::agent::AgentBuilder::new(model.erase())
        .add_hook(hook)
        .tool(rig::test_utils::MockAddTool)
        .build();
    let mut stream = agent.prompt("run").max_turns(16).stream();
    while stream.next().await.is_some() {}
    Ok(())
}

/// Drive one turn on `hook` against `model`, with a tool that always fails
/// with `kind` registered.
async fn drive_failing(
    hook: HookChain<AllowAllResolver>,
    model: MockCompletionModel,
    kind: rig::tool::ToolErrorKind,
) -> Result<()> {
    let agent = rig::agent::AgentBuilder::new(model.erase())
        .add_hook(hook)
        .tool(rig::test_utils::MockFailingTool::new(kind))
        .build();
    let mut stream = agent.prompt("run").max_turns(16).stream();
    while stream.next().await.is_some() {}
    Ok(())
}

/// A scripted turn whose first model call emits `call` and whose second
/// answers the tool result.
fn tool_call_turn(call: MockStreamEvent) -> MockCompletionModel {
    MockCompletionModel::from_stream_turns([
        vec![call, MockStreamEvent::final_response_with_default_usage()],
        vec![
            MockStreamEvent::Text("tool done".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ])
}

/// The turn's dispatch and its outcome leave one record carrying the call
/// identity and the settled result.
#[tokio::test]
async fn turn_tools_records_a_dispatched_call_and_its_result() -> Result<()> {
    // -- Setup & Fixtures
    let model = tool_call_turn(MockStreamEvent::tool_call(
        "tc1",
        "add",
        serde_json::json!({"x": 1, "y": 2}),
    ));
    let ToolTurn { hook, turn_tools } = tool_turn(&model)?;

    // -- Exec
    drive_add(hook, model).await?;
    let records = turn_tools.records();

    // -- Check
    assert_eq!(records.len(), 1, "one tool call must yield one record");
    assert_eq!(records[0].name.as_str(), "add");
    assert_eq!(records[0].call_id.to_string(), "tc1");
    let arguments: serde_json::Value = serde_json::from_str(&records[0].arguments)?;
    assert_eq!(
        arguments,
        serde_json::json!({"x": 1, "y": 2}),
        "the record must carry the dispatch's arguments"
    );
    assert_eq!(
        records[0].result.as_deref(),
        Some("3"),
        "the record must carry the rendered result"
    );
    assert_eq!(records[0].success, Some(true));
    assert_eq!(records[0].error_kind, None);
    Ok(())
}

/// A failed tool result records the structural failure and its error kind.
#[tokio::test]
async fn turn_tools_records_a_failed_result_with_its_error_kind() -> Result<()> {
    // -- Setup & Fixtures
    let model = tool_call_turn(MockStreamEvent::tool_call(
        "tc1",
        "flaky_tool",
        serde_json::json!({}),
    ));
    let ToolTurn { hook, turn_tools } = tool_turn(&model)?;

    // -- Exec
    drive_failing(hook, model, rig::tool::ToolErrorKind::Timeout).await?;
    let records = turn_tools.records();

    // -- Check
    assert_eq!(records.len(), 1, "one tool call must yield one record");
    assert_eq!(records[0].name.as_str(), "flaky_tool");
    assert_eq!(
        records[0].success,
        Some(false),
        "a failed result must record a false success verdict"
    );
    assert_eq!(
        records[0].error_kind.as_deref(),
        Some("timeout"),
        "the record must carry the result's error kind"
    );
    Ok(())
}

/// A turn that calls no tool records nothing.
#[tokio::test]
async fn turn_tools_stay_empty_when_no_tool_fires() -> Result<()> {
    // -- Setup & Fixtures
    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("done".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);
    let ToolTurn { hook, turn_tools } = tool_turn(&model)?;

    // -- Exec
    drive_add(hook, model).await?;

    // -- Check
    assert!(
        turn_tools.records().is_empty(),
        "a turn with no tool call must record nothing"
    );
    Ok(())
}
