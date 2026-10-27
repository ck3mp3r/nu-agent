//! The cancel arm's `Started` + `Completed(success=false)` emission.
//!
//! Two tests: the direct helper test (the arm's emission cannot be driven
//! through `on_dispatch` alone — rig's `HookContext::new` is `pub(crate)`), and
//! the real-turn wiring test that proves the arm calls the helper and leaves
//! the denied call's record untouched.

use super::*;

use rig::tool::Tool;

/// Resolver that allows every call: the wiring test drives the cancel arm, not
/// the permission flow.
#[derive(Clone)]
struct AllowAllResolver;

impl crate::hook::permission_resolver::AsyncPermissionResolver for AllowAllResolver {
    async fn resolve(
        &self,
        _tool_name: &str,
        _arguments: &str,
        _tool_call_id: Option<String>,
        _bus: &Bus,
    ) -> crate::hook::permission_resolver::PermissionDecision {
        crate::hook::permission_resolver::PermissionDecision::Allow
    }
}

/// A tool whose body requests a cancel, so the NEXT dispatched call meets the
/// cancel arm. `tool_concurrency` is 1, so the batch's calls run strictly in
/// call order: this call's body publishes the cancel, then the sibling's
/// dispatch consumes it.
struct CancelSenderTool {
    bus: Bus,
}

impl rig::tool::Tool for CancelSenderTool {
    const NAME: &'static str = "cancel_sender";
    type Args = serde_json::Value;
    type Output = String;
    type Error = std::convert::Infallible;

    fn description(&self) -> String {
        "Requests a turn cancel".to_string()
    }

    fn parameters(&self) -> serde_json::Value {
        serde_json::json!({"type": "object", "properties": {}})
    }

    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        _args: Self::Args,
    ) -> core::result::Result<Self::Output, Self::Error> {
        let _ = self.bus.cancel().request_cancel().await;
        Ok("cancel requested".to_string())
    }
}

/// The policy resolver the direct helper test builds its chain with.
fn policy_resolver() -> PolicyPermissionResolver {
    PolicyPermissionResolver {
        permissions: Arc::new(crate::tools::authz::PermissionsConfig::safe_defaults(false)),
        session_grants: Arc::new(Mutex::new(crate::tools::authz::SessionGrantCache::default())),
        closure_registry: Arc::new(crate::tools::closure::ClosureRegistry::default()),
        mcp_registry: Arc::new(crate::tools::handler::McpToolRegistry::empty()),
    }
}

/// Build a hook chain over `bus` with no tools registered and no scripted
/// model: neither test needs them.
fn build_hook<P: crate::hook::permission_resolver::AsyncPermissionResolver>(
    bus: Bus,
    permission: P,
) -> HookChain<P> {
    let memory = Arc::new(CachedMemory::new(
        crate::session::RigStoreConfig::in_memory(),
    ));
    HookChain::new(
        HookEnv {
            bus,
            cwd: std::path::PathBuf::from("/tmp"),
        },
        permission,
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
            conversation_id: "cancel-dispatch-conv".to_string(),
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
            model: MockCompletionModel::default().erase(),
            last_total_tokens: Arc::new(Mutex::new(None)),
            turn_usage: crate::hook::TurnUsage::default(),
            session_usage: crate::hook::TurnUsage::default(),
        },
    )
}

/// The cancel arm's emission publishes `ToolStarted` then
/// `ToolCompleted(success=false)` on the ui_event channel, in that order, with
/// the cancelled-dispatch message — the pair the live transcript needs to
/// render the ⊘ block for a call the stop denied before it ran.
#[tokio::test]
async fn emit_cancelled_dispatch_publishes_started_then_failed_completed() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    // Build the hook first: it subscribes to the cancel channel in `new`, and
    // broadcast sends are not buffered for later subscribers.
    let hook = build_hook(bus.clone(), policy_resolver());
    let mut ui_rx = bus.ui_event().subscribe();
    bus.cancel()
        .request_cancel()
        .await
        .map_err(|_| "should publish the cancel")?;

    // -- Exec
    hook.emit_cancelled_dispatch("read", "builtin", "{\"path\":\"/tmp/f\"}")
        .await;

    // -- Check
    let started = ui_rx
        .try_recv()
        .map_err(|_| "should receive ToolStarted first")?;
    assert_eq!(
        started,
        UiEvent::ToolStarted {
            name: "read".to_string(),
            source: "builtin".to_string(),
            arguments: "{\"path\":\"/tmp/f\"}".to_string(),
            call_line: crate::protocol::tool_args::CallLine {
                summary: "→ /tmp/f".to_string(),
            },
        },
        "the denied call must publish ToolStarted with the registry-rendered call line"
    );

    let completed = ui_rx
        .try_recv()
        .map_err(|_| "should receive ToolCompleted second")?;
    assert_eq!(
        completed,
        UiEvent::ToolCompleted {
            name: "read".to_string(),
            source: "builtin".to_string(),
            arguments: "{\"path\":\"/tmp/f\"}".to_string(),
            success: false,
            result: String::new(),
            display: None,
            error_kind: Some("cancelled".to_string()),
            message: Some("Interrupted: the turn stopped before this call ran".to_string()),
        },
        "the denied call must publish a failed ToolCompleted carrying the cancelled-dispatch message"
    );
    Ok(())
}

/// The tool-event shape the wiring assertions compare: `(name, success,
/// message)`, with `success` and `message` `None` for a `ToolStarted`.
fn tool_event_summary(event: &UiEvent) -> Option<(String, Option<bool>, Option<String>)> {
    match event {
        UiEvent::ToolStarted { name, .. } => Some((name.clone(), None, None)),
        UiEvent::ToolCompleted {
            name,
            success,
            message,
            ..
        } => Some((name.clone(), Some(*success), message.clone())),
        _ => None,
    }
}

/// The cancel arm fires end-to-end: a scripted batch's first call runs and its
/// body requests a cancel, the second call's dispatch consumes it and hits the
/// arm, so the second call publishes the `Started` + `Completed(success=false)`
/// pair and the run ends with the cancel error. The denied call's record keeps
/// `result`, `success`, and `error_kind` at `None` — the emission is a UI event,
/// not a settled result.
#[tokio::test]
async fn on_dispatch_cancel_arm_emits_pair_for_the_denied_call() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::default();
    let hook = build_hook(bus.clone(), AllowAllResolver);
    // Clone the log before `add_hook` consumes the hook.
    let turn_tools = hook.turn_tools();
    let model = MockCompletionModel::from_stream_turns([vec![
        MockStreamEvent::tool_call("tc1", CancelSenderTool::NAME, serde_json::json!({})),
        MockStreamEvent::tool_call("tc2", "add", serde_json::json!({"x": 1, "y": 2})),
        MockStreamEvent::final_response_with_default_usage(),
    ]])
    .erase();
    let agent = rig::agent::AgentBuilder::new(model)
        .add_hook(hook)
        .tool(CancelSenderTool { bus: bus.clone() })
        .tool(rig::test_utils::MockAddTool)
        .build();
    // Subscribe BEFORE driving the stream — broadcast sends are not buffered
    // for later subscribers.
    let mut ui_rx = bus.ui_event().subscribe();

    // -- Exec: drive the batch; tc1's body publishes the cancel, tc2's dispatch
    // consumes it.
    let mut items = Vec::new();
    let mut stream = agent.prompt("run").max_turns(16).stream();
    while let Some(item) = stream.next().await {
        items.push(item);
    }

    // -- Check: tc1's pair precedes tc2's, and tc2's pair is the failed one.
    let mut tool_events = Vec::new();
    while let Ok(event) = ui_rx.try_recv() {
        if let Some(summary) = tool_event_summary(&event) {
            tool_events.push(summary);
        }
    }
    assert_eq!(
        tool_events,
        vec![
            (CancelSenderTool::NAME.to_string(), None, None),
            (CancelSenderTool::NAME.to_string(), Some(true), None),
            ("add".to_string(), None, None),
            (
                "add".to_string(),
                Some(false),
                Some(CANCELLED_DISPATCH_MESSAGE.to_string()),
            ),
        ],
        "the denied call must publish its Started+Completed pair after the call that ran"
    );

    // -- Check: the run ends with the cancel error.
    let last = items
        .pop()
        .ok_or("the stream must yield at least one item")?;
    let error = last.err().ok_or("the run must end with the cancel error")?;
    let rig::completion::PromptError::Cancelled { reason, .. } = error else {
        return Err("the run must end with Cancelled".into());
    };
    assert_eq!(reason, "Cancelled by user");

    // -- Check: the denied call's record keeps no settled result.
    let records = turn_tools.records();
    assert_eq!(records.len(), 2, "both dispatched calls must be recorded");
    let ran = records
        .iter()
        .find(|record| record.call_id.to_string() == "tc1")
        .ok_or("the first call must be recorded")?;
    assert_eq!(
        ran.success,
        Some(true),
        "the call that ran must settle successfully"
    );
    let denied = records
        .iter()
        .find(|record| record.call_id.to_string() == "tc2")
        .ok_or("the denied call must be recorded")?;
    assert_eq!(denied.name, ToolName::new("add")?);
    assert_eq!(denied.result, None, "the denied call must keep no result");
    assert_eq!(
        denied.success, None,
        "the denied call must keep no success verdict"
    );
    assert_eq!(
        denied.error_kind, None,
        "the denied call must keep no error kind"
    );
    Ok(())
}
