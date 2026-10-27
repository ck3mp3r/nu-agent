use super::*;

use crate::hook::permission_resolver::permission_resolver_test::submit_decision;

#[test]
fn suppress_previewed_display_drops_it_when_already_shown() {
    // -- Setup & Fixtures
    let resolver = StubPreviewResolver { previewed: true };

    // -- Exec
    let result = suppress_previewed_display(&resolver, "edit", "{}", Some(sample_display()));

    // -- Check
    assert!(
        result.is_none(),
        "a display already shown via pre-authorize preview must be suppressed"
    );
}

#[test]
fn suppress_previewed_display_passes_through_when_not_previewed() {
    // -- Setup & Fixtures
    let resolver = StubPreviewResolver { previewed: false };
    let display = Some(sample_display());

    // -- Exec
    let result = suppress_previewed_display(&resolver, "edit", "{}", display.clone());

    // -- Check
    assert_eq!(
        result, display,
        "a display that was never previewed must pass through unchanged"
    );
}

#[test]
fn suppress_previewed_display_none_input_stays_none_either_way() {
    // -- Setup & Fixtures
    let previewed = StubPreviewResolver { previewed: true };
    let not_previewed = StubPreviewResolver { previewed: false };

    // -- Exec & Check
    for resolver in [previewed, not_previewed] {
        assert!(
            suppress_previewed_display(&resolver, "nu", "{}", None).is_none(),
            "a tool with no display to begin with must stay None regardless of previewed state"
        );
    }
}

// ---------------------------------------------------------------------------
// Producer-side FIFO invariant: ToolStarted must be published before
// PermissionRequested on the single ui_event channel.
// ---------------------------------------------------------------------------

/// Regression test for the producer-side FIFO invariant that the unified UI
/// event loop depends on: `HookChain::on_dispatch` publishes
/// `UiEvent::ToolStarted` BEFORE the permission resolver publishes
/// `UiEvent::PermissionRequested`. Both travel on the single `bus.ui_event()`
/// channel, so arrival order equals send order.
///
/// If a future edit moves `permission.resolve()` above the `ToolStarted` send
/// (chain.rs), the edit diff preview renders above the tool-call line again —
/// this test fails.
#[tokio::test]
async fn on_dispatch_publishes_tool_started_before_permission_requested() -> Result<()> {
    // -- Setup & Fixtures
    let temp_dir = TempDir::new().map_err(|_| "should create temp dir")?;
    let memory = Arc::new(CachedMemory::new(
        crate::session::RigStoreConfig::in_memory(),
    ));
    let bus = Bus::default();
    let pending = Arc::new(Mutex::new(std::collections::HashMap::new()));
    let resolver = InteractivePermissionResolver::new(
        Arc::clone(&pending),
        Arc::new(crate::tools::authz::PermissionsConfig::safe_defaults(true)),
        Arc::new(Mutex::new(crate::tools::authz::SessionGrantCache::default())),
        Arc::new(crate::tools::closure::ClosureRegistry::default()),
        Arc::new(crate::tools::handler::McpToolRegistry::empty()),
        temp_dir.path().to_path_buf(),
    );
    let resolver_clone = resolver.clone();
    // The scripted tool-call turn is queued on the shared model; the agent is
    // built from it below.
    let shared_model = Arc::new(Mutex::new(
        MockCompletionModel::from_stream_turns([
            vec![
                MockStreamEvent::tool_call("tc1", "add", serde_json::json!({"x": 1, "y": 2})),
                MockStreamEvent::final_response_with_default_usage(),
            ],
            vec![
                MockStreamEvent::Text("tool done".to_string()),
                MockStreamEvent::final_response_with_default_usage(),
            ],
        ])
        .erase(),
    ));
    let hook = HookChain::new(
        HookEnv {
            bus: bus.clone(),
            cwd: std::path::PathBuf::from("/tmp"),
        },
        resolver,
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
            repetition_guard: true,
            memory,
            conversation_id: "fifo-invariant-conv".to_string(),
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
            model: shared_model.lock().expect("model mutex poisoned").clone(),
            last_total_tokens: Arc::new(Mutex::new(None)),
            turn_usage: crate::hook::TurnUsage::default(),
            session_usage: crate::hook::TurnUsage::default(),
        },
    );
    // The "add" tool is not in `safe_defaults(true)`'s allow list, so the
    // global Ask action fires and the resolver publishes PermissionRequested.
    let agent =
        rig::agent::AgentBuilder::new(shared_model.lock().expect("model mutex poisoned").clone())
            .add_hook(hook)
            .tool(rig::test_utils::MockAddTool)
            .build();
    // Subscribe BEFORE driving the stream — broadcast sends are not buffered
    // for later subscribers.
    let mut ui_rx = bus.ui_event().subscribe();

    // -- Exec: drive the turn; the tool call blocks on the permission oneshot.
    let turn = tokio::spawn(async move {
        let mut stream = agent.prompt("run").max_turns(16).stream();
        while stream.next().await.is_some() {}
    });

    // Collect ui_event arrivals until PermissionRequested, then submit a
    // decision so the blocked resolve() future can continue.
    let mut order = Vec::new();
    let request_id = loop {
        let event = tokio::time::timeout(Duration::from_secs(5), ui_rx.recv())
            .await
            .map_err(|_| "should receive a ui_event within timeout")?
            .map_err(|_| "ui_event channel should stay open")?;
        match event {
            UiEvent::ToolStarted { .. } => order.push("ToolStarted"),
            UiEvent::PermissionRequested { request_id, .. } => {
                order.push("PermissionRequested");
                break request_id;
            }
            _ => {}
        }
    };
    submit_decision(
        &resolver_clone,
        &request_id,
        ProtocolPermissionDecision::AllowOnce,
    );
    turn.await.map_err(|_| "turn task should not panic")?;

    // -- Check
    assert_eq!(
        order,
        vec!["ToolStarted", "PermissionRequested"],
        "ToolStarted must arrive before PermissionRequested on the FIFO ui_event channel"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// ToolRenderRegistry wiring: on_dispatch renders the call line via the
// registry rather than passing raw arguments through.
// ---------------------------------------------------------------------------

/// `HookChain::on_dispatch` consults its `ToolRenderRegistry` and publishes
/// the tailored `CallLine` on `UiEvent::ToolStarted`. A registered
/// `read` tool renders `→ /tmp/f`; the raw JSON arguments never reach the
/// event.
#[tokio::test]
async fn on_dispatch_publishes_registry_rendered_call_line() -> Result<()> {
    // -- Setup & Fixtures
    let memory = Arc::new(CachedMemory::new(
        crate::session::RigStoreConfig::in_memory(),
    ));
    let bus = Bus::default();
    let mut render_registry = crate::tools::handler::builtin_tool::ToolRenderRegistry::default();
    render_registry.register(
        "read",
        crate::tools::handler::read::ReadTool::call_line_render,
    );
    let shared_model = Arc::new(Mutex::new(
        MockCompletionModel::from_stream_turns([
            vec![
                MockStreamEvent::tool_call("tc1", "read", serde_json::json!({"path": "/tmp/f"})),
                MockStreamEvent::final_response_with_default_usage(),
            ],
            vec![
                MockStreamEvent::Text("done".to_string()),
                MockStreamEvent::final_response_with_default_usage(),
            ],
        ])
        .erase(),
    ));
    let hook = HookChain::new(
        HookEnv {
            bus: bus.clone(),
            cwd: std::path::PathBuf::from("/tmp"),
        },
        PolicyPermissionResolver {
            permissions: Arc::new(crate::tools::authz::PermissionsConfig::safe_defaults(false)),
            session_grants: Arc::new(Mutex::new(crate::tools::authz::SessionGrantCache::default())),
            closure_registry: Arc::new(crate::tools::closure::ClosureRegistry::default()),
            mcp_registry: Arc::new(crate::tools::handler::McpToolRegistry::empty()),
        },
        Arc::new(crate::tools::closure::ClosureRegistry::default()),
        Arc::new(crate::tools::handler::McpToolRegistry::empty()),
        render_registry,
        None,
        HookState {
            circuit_breaker: Arc::new(Mutex::new(
                crate::tools::mcp::circuit_breaker::McpCircuitBreaker::default(),
            )),
            doom_state: Arc::new(Mutex::new(DoomLoopState::default())),
            output_repetition: Arc::new(Mutex::new(RepetitionState::default())),
            repetition_guard: true,
            memory,
            conversation_id: "render-registry-conv".to_string(),
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
            model: shared_model.lock().expect("model mutex poisoned").clone(),
            last_total_tokens: Arc::new(Mutex::new(None)),
            turn_usage: crate::hook::TurnUsage::default(),
            session_usage: crate::hook::TurnUsage::default(),
        },
    );
    let agent =
        rig::agent::AgentBuilder::new(shared_model.lock().expect("model mutex poisoned").clone())
            .add_hook(hook)
            .dynamic_tool(rig::tool::DynamicTool::new(
                ToolName::new("read")?,
                "Read a file",
                serde_json::json!({"type": "object", "properties": {"path": {"type": "string"}}}),
                |_args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("contents")) }),
            ))
            .build();
    // Subscribe BEFORE driving the stream — broadcast sends are not buffered
    // for later subscribers.
    let mut ui_rx = bus.ui_event().subscribe();

    // -- Exec: drive the turn and capture the first ToolStarted event.
    let turn = tokio::spawn(async move {
        let mut stream = agent.prompt("run").max_turns(16).stream();
        while stream.next().await.is_some() {}
    });
    let call_line = loop {
        let event = tokio::time::timeout(Duration::from_secs(5), ui_rx.recv())
            .await
            .map_err(|_| "should receive a ui_event within timeout")?
            .map_err(|_| "ui_event channel should stay open")?;
        if let UiEvent::ToolStarted { call_line, .. } = event {
            break call_line;
        }
    };
    turn.await.map_err(|_| "turn task should not panic")?;

    // -- Check
    assert_eq!(
        call_line,
        crate::protocol::tool_args::CallLine {
            summary: "→ /tmp/f".to_string(),
        },
        "ToolStarted must carry the registry-rendered call line"
    );
    Ok(())
}
