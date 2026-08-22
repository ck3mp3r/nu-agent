// Tests for CommandRouter agent-switch dispatch through the interactive loop.
//
// These verify that SwitchAgent commands dispatched by CommandRouter on the
// worker thread produce the correct UI updates when the orchestrator loop
// processes the response channel.

use nu_protocol::{LabeledError, Span, Value};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::bus::create_bus;
use crate::orchestrator::{
    CommandRouter, InteractiveLoopConfig, OnAgentSwitch, OrchestratorEvent, UiRequest,
    UiRequestResponse, WorkerCommand, run_interactive_loop_impl,
};
use crate::protocol::{
    compaction::{CompactionTriggerDecision, CompactionTriggerSource},
    compaction_runtime::Compaction,
    contracts::{CoreRuntime, McpUsabilityState, ProgressUi, UiMessageSnapshot, UserInputUi},
    event::UiEvent,
    mcp_management::McpManagement,
    model_switching::ModelSwitching,
    session_management::{SessionPersistence, SessionState},
};

// ---------------------------------------------------------------------------
// Simple fake runtime with switch_agent support (idle worker)
// ---------------------------------------------------------------------------

struct AgentSwitchRuntime {
    switched_agents: Vec<String>,
    switch_agent_result: Option<Result<String, String>>,
    active_model: String,
}

impl Default for AgentSwitchRuntime {
    fn default() -> Self {
        Self {
            switched_agents: Vec::new(),
            switch_agent_result: None,
            active_model: "openai/gpt-4o-mini".to_string(),
        }
    }
}

impl CoreRuntime for AgentSwitchRuntime {
    async fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        ui.emit(&UiEvent::Completed { tool_calls: 0 });
        Ok(Value::nothing(span))
    }
}

impl McpManagement for AgentSwitchRuntime {
    async fn set_mcp_server_enabled(
        &mut self,
        _name: &str,
        _enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(McpUsabilityState::Disabled)
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        0
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        0
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        Vec::new()
    }
}

impl ModelSwitching for AgentSwitchRuntime {
    fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    fn switch_agent(&mut self, agent_name: &str) -> Result<String, String> {
        self.switched_agents.push(agent_name.to_string());
        if let Some(result) = self.switch_agent_result.clone() {
            return result;
        }
        Ok(agent_name.to_string())
    }

    fn active_model_identity(&self) -> String {
        self.active_model.clone()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

impl SessionState for AgentSwitchRuntime {
    fn clear_session(&mut self) {}
    fn new_session(&mut self) {}
    fn seed_last_total_tokens(&mut self, _tokens: Option<u64>) {}
}
impl SessionPersistence for AgentSwitchRuntime {}

impl Compaction for AgentSwitchRuntime {
    fn evaluate_auto_compaction(&mut self) -> Option<CompactionTriggerDecision> {
        None
    }

    async fn execute_compaction_trigger<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _source: CompactionTriggerSource,
    ) -> Result<(), String> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Simple fake interactive UI with agent switch request support
// ---------------------------------------------------------------------------

struct AgentSwitchUi {
    event_tx: tokio::sync::mpsc::Sender<OrchestratorEvent>,
    agent_switch_requests: Arc<Mutex<std::collections::VecDeque<String>>>,
    quit: Arc<AtomicBool>,
    warnings: Arc<Mutex<Vec<String>>>,
    active_model_identity: Arc<Mutex<Option<String>>>,
    active_agent_identity: Arc<Mutex<Option<String>>>,
    _bus_task: Option<tokio::task::JoinHandle<()>>,
}

impl AgentSwitchUi {
    fn new(agent_switches: &[&str]) -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
        Self {
            event_tx,
            agent_switch_requests: Arc::new(Mutex::new(
                agent_switches.iter().map(|s| s.to_string()).collect(),
            )),
            quit: Arc::new(AtomicBool::new(false)),
            warnings: Arc::new(Mutex::new(Vec::new())),
            active_model_identity: Arc::new(Mutex::new(None)),
            active_agent_identity: Arc::new(Mutex::new(None)),
            _bus_task: None,
        }
    }

    fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let quit = Arc::clone(&self.quit);
        let agent_switch_requests = Arc::clone(&self.agent_switch_requests);
        let warnings = Arc::clone(&self.warnings);
        let active_model_identity = Arc::clone(&self.active_model_identity);
        let active_agent_identity = Arc::clone(&self.active_agent_identity);

        let mut turn_rx = bus.turn().subscribe();
        let mut warning_rx = bus.warning().subscribe();
        let mut ui_state_rx = bus.ui_state().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = turn_rx.recv() => {
                        if let crate::bus::TurnEvent::TurnCompleted { .. } = event
                            && agent_switch_requests
                                .lock()
                                .expect("agent switch requests lock")
                                .is_empty()
                        {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    ok = warning_rx.recv() => {
                        if let Ok(crate::bus::WarningEvent::Message { message }) = ok {
                            warnings.lock().expect("warnings lock").push(message);
                        }
                    }
                    ok = ui_state_rx.recv() => {
                        if let Ok(event) = ok {
                            match event {
                                crate::orchestrator::UiStateEvent::SetActiveModelIdentity(identity) => {
                                    *active_model_identity.lock().expect("active model identity lock") =
                                        Some(identity);
                                }
                                crate::orchestrator::UiStateEvent::SetActiveAgentIdentity(identity) => {
                                    *active_agent_identity.lock().expect("active agent identity lock") =
                                        Some(identity);
                                }
                                _ => {}
                            }
                        }
                    }
                    else => break,
                }
            }
        }));
        self
    }

    /// Feeds the queued agent-switch requests into the orchestrator's event
    /// channel and sends `Quit` once all have been dispatched.
    fn make_event_spawner(&self) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let agent_switch_requests = Arc::clone(&self.agent_switch_requests);
        move |event_tx| {
            let event_tx = event_tx.clone();
            tokio::spawn(async move {
                loop {
                    let name = {
                        let mut guard = agent_switch_requests
                            .lock()
                            .expect("agent switch requests lock");
                        guard.pop_front()
                    };
                    let Some(name) = name else {
                        break;
                    };
                    let _ = event_tx
                        .send(OrchestratorEvent::UiRequest(UiRequest::SwitchAgent {
                            name,
                        }))
                        .await;
                }
                let _ = event_tx.send(OrchestratorEvent::Quit).await;
            });
        }
    }
}

impl UserInputUi for AgentSwitchUi {
    fn event_sender(&self) -> &tokio::sync::mpsc::Sender<OrchestratorEvent> {
        &self.event_tx
    }
}

// ---------------------------------------------------------------------------
// Long-running runtime with agent switch support (for queuing tests)
// ---------------------------------------------------------------------------

struct LongRunningAgentRuntime {
    prompts: Arc<Mutex<Vec<String>>>,
    switched_agents: Arc<Mutex<Vec<String>>>,
    active: Arc<AtomicBool>,
    block_first_turn: Arc<AtomicBool>,
    active_model: String,
}

impl LongRunningAgentRuntime {
    fn new(block_first_turn: Arc<AtomicBool>) -> Self {
        Self {
            prompts: Arc::new(Mutex::new(Vec::new())),
            switched_agents: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(AtomicBool::new(false)),
            block_first_turn,
            active_model: "openai/gpt-4o-mini".to_string(),
        }
    }
}

impl CoreRuntime for LongRunningAgentRuntime {
    async fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.active.store(true, Ordering::SeqCst);
        self.prompts
            .lock()
            .expect("prompts lock")
            .push(prompt.clone());

        if prompt == "first" {
            while !self.block_first_turn.load(Ordering::SeqCst) {
                if ui.take_cancel_requested() {
                    self.active.store(false, Ordering::SeqCst);
                    return Err(LabeledError::new("LLM call cancelled"));
                }
                ui.emit(&UiEvent::Tick);
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }

        ui.emit(&UiEvent::Completed { tool_calls: 0 });
        self.active.store(false, Ordering::SeqCst);
        Ok(Value::nothing(Span::test_data()))
    }
}

impl McpManagement for LongRunningAgentRuntime {
    async fn set_mcp_server_enabled(
        &mut self,
        _name: &str,
        _enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(McpUsabilityState::Disabled)
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        0
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        0
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        Vec::new()
    }
}

impl ModelSwitching for LongRunningAgentRuntime {
    fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    fn switch_agent(&mut self, agent_name: &str) -> Result<String, String> {
        self.switched_agents
            .lock()
            .expect("switched agents lock")
            .push(agent_name.to_string());
        Ok(agent_name.to_string())
    }

    fn active_model_identity(&self) -> String {
        self.active_model.clone()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

impl SessionState for LongRunningAgentRuntime {
    fn clear_session(&mut self) {}
    fn new_session(&mut self) {}
    fn seed_last_total_tokens(&mut self, _tokens: Option<u64>) {}
}
impl SessionPersistence for LongRunningAgentRuntime {}

impl Compaction for LongRunningAgentRuntime {
    fn evaluate_auto_compaction(&mut self) -> Option<CompactionTriggerDecision> {
        None
    }

    async fn execute_compaction_trigger<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _source: CompactionTriggerSource,
    ) -> Result<(), String> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Responsive interactive UI that injects agent switch requests while busy
// ---------------------------------------------------------------------------

struct ResponsiveAgentSwitchUi {
    event_tx: tokio::sync::mpsc::Sender<OrchestratorEvent>,
    submitted: std::collections::VecDeque<String>,
    agent_switch_requests: std::collections::VecDeque<String>,
    quit: Arc<AtomicBool>,
    completed_count: Arc<Mutex<usize>>,
    expected_completions: usize,
    warnings: Arc<Mutex<Vec<String>>>,
    active_agent_identity: Arc<Mutex<Option<String>>>,
    _turn_task: Option<tokio::task::JoinHandle<()>>,
}

impl ResponsiveAgentSwitchUi {
    fn new(
        initial_prompts: &[&str],
        injected_agent_switch_during_active: &[&str],
        expected_completions: usize,
    ) -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
        Self {
            event_tx,
            submitted: initial_prompts.iter().map(|s| s.to_string()).collect(),
            agent_switch_requests: injected_agent_switch_during_active
                .iter()
                .map(|s| s.to_string())
                .collect(),
            quit: Arc::new(AtomicBool::new(false)),
            completed_count: Arc::new(Mutex::new(0)),
            expected_completions,
            warnings: Arc::new(Mutex::new(Vec::new())),
            active_agent_identity: Arc::new(Mutex::new(None)),
            _turn_task: None,
        }
    }

    fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let quit = Arc::clone(&self.quit);
        let completed_count = Arc::clone(&self.completed_count);
        let expected_completions = self.expected_completions;
        let warnings = Arc::clone(&self.warnings);
        let active_agent_identity = Arc::clone(&self.active_agent_identity);

        let mut turn_rx = bus.turn().subscribe();
        let mut warning_rx = bus.warning().subscribe();
        let mut ui_state_rx = bus.ui_state().subscribe();

        self._turn_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = turn_rx.recv() => {
                        if let crate::bus::TurnEvent::TurnCompleted { .. } = event {
                            let count = {
                                let mut count = completed_count.lock().expect("completed count lock");
                                *count += 1;
                                *count
                            };
                            if count >= expected_completions {
                                quit.store(true, Ordering::SeqCst);
                            }
                        }
                    }
                    Ok(event) = warning_rx.recv() => {
                        if let crate::bus::WarningEvent::Message { message } = event {
                            warnings.lock().expect("warnings lock").push(message);
                        }
                    }
                    Ok(event) = ui_state_rx.recv() => {
                        if let crate::orchestrator::UiStateEvent::SetActiveAgentIdentity(identity) = event {
                            *active_agent_identity.lock().expect("active agent identity lock") = Some(identity);
                        }
                    }
                    else => break,
                }
            }
        }));
        self
    }

    /// Feeds the first prompt immediately, then injects agent-switch requests
    /// only while the worker is active. `Quit` is gated on the expected turn
    /// completion count (set by `with_bus`).
    fn make_event_spawner(&self) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let submitted = self.submitted.clone();
        let agent_switch_requests = self.agent_switch_requests.clone();
        let quit = Arc::clone(&self.quit);
        move |event_tx| {
            let event_tx = event_tx.clone();
            let mut submitted = submitted.clone();
            let mut agent_switch_requests = agent_switch_requests.clone();
            tokio::spawn(async move {
                if let Some(prompt) = submitted.pop_front() {
                    let _ = event_tx
                        .send(OrchestratorEvent::PromptSubmitted { text: prompt })
                        .await;
                }
                while !quit.load(Ordering::SeqCst) {
                    if let Some(name) = agent_switch_requests.pop_front() {
                        let _ = event_tx
                            .send(OrchestratorEvent::UiRequest(UiRequest::SwitchAgent {
                                name,
                            }))
                            .await;
                    }
                    if quit.load(Ordering::SeqCst)
                        && event_tx.send(OrchestratorEvent::Quit).await.is_err()
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                let _ = event_tx.send(OrchestratorEvent::Quit).await;
            });
        }
    }
}

impl UserInputUi for ResponsiveAgentSwitchUi {
    fn event_sender(&self) -> &tokio::sync::mpsc::Sender<OrchestratorEvent> {
        &self.event_tx
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_switch_sends_command_and_updates_ui_identity() {
    let bus = create_bus();
    let runtime = AgentSwitchRuntime::default();
    let ui = AgentSwitchUi::new(&["research-agent"]).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.switched_agents, vec!["research-agent"]);
    assert_eq!(
        *ui.active_agent_identity
            .lock()
            .expect("active agent identity lock"),
        Some("research-agent".to_string())
    );
    assert_eq!(
        *ui.active_model_identity
            .lock()
            .expect("active model identity lock"),
        Some("openai/gpt-4o-mini".to_string())
    );
}

#[tokio::test]
async fn agent_switch_failure_warns_and_keeps_previous_agent() {
    let bus = create_bus();
    let runtime = AgentSwitchRuntime {
        switch_agent_result: Some(Err("agent not found".to_string())),
        ..Default::default()
    };
    let ui = AgentSwitchUi::new(&["nonexistent-agent"]).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert!(
        ui.warnings
            .lock()
            .expect("warnings lock")
            .iter()
            .any(|w| w == "agent not found")
    );
    assert_eq!(
        *ui.active_agent_identity
            .lock()
            .expect("active agent identity lock"),
        None
    );
}

#[tokio::test]
async fn agent_switch_while_worker_active_is_queued_for_next_turn() {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningAgentRuntime::new(Arc::clone(&block_first_turn));
    let ui = ResponsiveAgentSwitchUi::new(&["first"], &["research-agent"], 1).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    // Spawn a background task to unblock the turn after a delay.
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        unblock.store(true, Ordering::SeqCst);
    });

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        _runtime
            .switched_agents
            .lock()
            .expect("switched agents lock")
            .as_slice(),
        ["research-agent"]
    );
    assert!(
        ui.warnings
            .lock()
            .expect("warnings lock")
            .iter()
            .any(|w| w == "Agent switch queued for next turn: research-agent")
    );
    assert_eq!(
        *ui.active_agent_identity
            .lock()
            .expect("active agent identity lock"),
        Some("research-agent".to_string())
    );
}

#[tokio::test]
async fn queued_agent_switch_last_write_wins() {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningAgentRuntime::new(Arc::clone(&block_first_turn));
    let ui =
        ResponsiveAgentSwitchUi::new(&["first"], &["agent-a", "agent-b"], 1).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    // Spawn a background task to unblock the turn after a delay.
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        unblock.store(true, Ordering::SeqCst);
    });

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        _runtime
            .switched_agents
            .lock()
            .expect("switched agents lock")
            .as_slice(),
        ["agent-b"]
    );
    assert_eq!(
        *ui.active_agent_identity
            .lock()
            .expect("active agent identity lock"),
        Some("agent-b".to_string())
    );
}

// ---------------------------------------------------------------------------
// SwitchSession runtime — tracks load_session calls to verify async dispatch
// ---------------------------------------------------------------------------

struct SwitchSessionRuntime {
    loaded_session_id: Arc<Mutex<Option<String>>>,
    active_model: String,
}

impl CoreRuntime for SwitchSessionRuntime {
    async fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        ui.emit(&UiEvent::Completed { tool_calls: 0 });
        Ok(Value::nothing(span))
    }
}

impl ModelSwitching for SwitchSessionRuntime {
    fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported".to_string())
    }

    fn active_model_identity(&self) -> String {
        self.active_model.clone()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

impl SessionState for SwitchSessionRuntime {
    fn clear_session(&mut self) {}
    fn new_session(&mut self) {}
    fn seed_last_total_tokens(&mut self, _tokens: Option<u64>) {}
}

impl SessionPersistence for SwitchSessionRuntime {
    async fn load_session(&mut self, session_id: &str) -> Result<Vec<UiMessageSnapshot>, String> {
        self.loaded_session_id
            .lock()
            .expect("loaded_session_id lock")
            .replace(session_id.to_string());
        Err("Session loading not supported".to_string())
    }
}

crate::default_mcp!(SwitchSessionRuntime);
crate::default_compaction!(SwitchSessionRuntime);

// ---------------------------------------------------------------------------
// Test: SwitchSession dispatches async load_session without panic
// ---------------------------------------------------------------------------

#[tokio::test]
async fn switch_session_dispatches_async_load_without_panic() {
    let loaded_session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let mut runtime = SwitchSessionRuntime {
        loaded_session_id: Arc::clone(&loaded_session_id),
        active_model: "openai/gpt-4o-mini".to_string(),
    };

    // Minimal ProgressUi — only needed to satisfy the trait bound on dispatch
    struct NoopProgressUi;
    impl ProgressUi for NoopProgressUi {
        fn emit(&mut self, _event: &UiEvent) {}
        fn flush(&mut self) {}
        fn take_cancel_requested(&self) -> bool {
            false
        }
    }
    let mut ui = NoopProgressUi;

    // result_tx is unused by SwitchSession but required by the dispatch signature
    let (result_tx, _result_rx) = tokio::sync::mpsc::channel(1);

    // Response channel for the SwitchSession command
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(1);

    let cmd = WorkerCommand::HandleUiRequest {
        request: UiRequest::SwitchSession {
            id: "test-session-123".to_string(),
        },
        response_tx,
    };

    // Dispatch from within an async context — this is the exact scenario
    // that was panicking with "Cannot start a runtime from within a runtime"
    // when load_session used Handle::current().block_on(...)
    let should_continue =
        CommandRouter::dispatch(cmd, &mut runtime, &mut ui, &result_tx, None).await;

    // 1. No panic occurred (reaching here proves it)
    assert!(should_continue, "SwitchSession should not trigger shutdown");

    // 2. load_session was called with the correct session_id
    {
        let loaded = loaded_session_id.lock().expect("lock");
        assert_eq!(
            loaded.as_deref(),
            Some("test-session-123"),
            "load_session should be called with the correct session_id"
        );
    }

    // 3. The result is sent through the response channel
    let response = response_rx.recv().await.expect("response should be sent");
    match response {
        UiRequestResponse::SessionSwitch { id, result } => {
            assert_eq!(id, "test-session-123");
            assert!(
                result.is_err(),
                "default load_session should return an error"
            );
        }
        other => panic!("unexpected response: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// on_agent_switch callback
// ---------------------------------------------------------------------------

#[tokio::test]
async fn on_agent_switch_callback_invoked_after_successful_switch() {
    let switched_name = Arc::new(std::sync::Mutex::new(None::<String>));
    let switched_desc = Arc::new(std::sync::Mutex::new(None::<Option<String>>));
    let switched_icon = Arc::new(std::sync::Mutex::new(None::<Option<String>>));
    let cb_name = Arc::clone(&switched_name);
    let cb_desc = Arc::clone(&switched_desc);
    let cb_icon = Arc::clone(&switched_icon);

    let callback: OnAgentSwitch = Arc::new(
        move |name: String, description: Option<String>, icon: Option<String>| {
            *cb_name.lock().expect("name lock") = Some(name);
            *cb_desc.lock().expect("desc lock") = Some(description);
            *cb_icon.lock().expect("icon lock") = Some(icon);
        },
    );

    let bus = create_bus();
    let runtime = AgentSwitchRuntime {
        switch_agent_result: Some(Ok("research-agent".to_string())),
        ..Default::default()
    };
    let ui = AgentSwitchUi::new(&["research-agent"]).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let config = InteractiveLoopConfig::new(Span::test_data())
        .with_on_agent_switch(callback)
        .with_bus(bus)
        .with_spawn_render_loop(spawner);

    let (_runtime, result) = run_interactive_loop_impl(runtime, config).await;
    let value = result.expect("interactive loop");
    assert!(value.is_nothing());

    // Verify the callback was invoked with the correct values
    let name = switched_name.lock().expect("name lock").take();
    let desc = switched_desc.lock().expect("desc lock").take();
    let icon = switched_icon.lock().expect("icon lock").take();
    assert_eq!(name.as_deref(), Some("research-agent"));
    // The AgentSwitchRuntime doesn't implement agent_description(), so it
    // returns None (the default trait method).
    assert_eq!(desc, Some(None));
    // The AgentSwitchRuntime doesn't implement agent_icon(), so it
    // returns None (the default trait method).
    assert_eq!(icon, Some(None));
}

// ---------------------------------------------------------------------------
// dispatch_ui_request tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dispatch_ui_request_switch_model() {
    let mut runtime = AgentSwitchRuntime::default();
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::SwitchModel {
            spec: "test-model".to_string(),
        },
        &mut runtime,
        &None,
        response_tx,
    )
    .await;

    let response = response_rx.recv().await.expect("response should be sent");
    match response {
        UiRequestResponse::ModelSwitch(result) => {
            assert!(
                result.is_err(),
                "AgentSwitchRuntime switch_model returns Err"
            );
        }
        other => panic!("expected ModelSwitch, got {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_ui_request_switch_agent() {
    let mut runtime = AgentSwitchRuntime::default();
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::SwitchAgent {
            name: "research-agent".to_string(),
        },
        &mut runtime,
        &None,
        response_tx,
    )
    .await;

    let response = response_rx.recv().await.expect("response should be sent");
    match response {
        UiRequestResponse::AgentSwitch(result) => {
            let (agent, model, _max_tokens, _icon) = result.expect("switch should succeed");
            assert_eq!(agent, "research-agent");
            assert_eq!(model, "openai/gpt-4o-mini");
        }
        other => panic!("expected AgentSwitch, got {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_ui_request_switch_agent_invokes_callback() {
    let switched_name = Arc::new(Mutex::new(None::<String>));
    let cb_name = Arc::clone(&switched_name);
    let callback: OnAgentSwitch = Arc::new(
        move |name: String, _desc: Option<String>, _icon: Option<String>| {
            *cb_name.lock().expect("name lock") = Some(name);
        },
    );

    let mut runtime = AgentSwitchRuntime::default();
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::SwitchAgent {
            name: "research-agent".to_string(),
        },
        &mut runtime,
        &Some(callback),
        response_tx,
    )
    .await;

    let response = response_rx.recv().await.expect("response should be sent");
    match response {
        UiRequestResponse::AgentSwitch(result) => {
            assert!(result.is_ok(), "switch should succeed");
        }
        other => panic!("expected AgentSwitch, got {other:?}"),
    }

    let name = switched_name.lock().expect("name lock").take();
    assert_eq!(name.as_deref(), Some("research-agent"));
}

#[tokio::test]
async fn dispatch_ui_request_switch_session() {
    let loaded_session_id: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let mut runtime = SwitchSessionRuntime {
        loaded_session_id: Arc::clone(&loaded_session_id),
        active_model: "openai/gpt-4o-mini".to_string(),
    };
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::SwitchSession {
            id: "test-session-123".to_string(),
        },
        &mut runtime,
        &None,
        response_tx,
    )
    .await;

    let response = response_rx.recv().await.expect("response should be sent");
    match response {
        UiRequestResponse::SessionSwitch { id, result } => {
            assert_eq!(id, "test-session-123");
            assert!(result.is_err(), "default load_session returns Err");
        }
        other => panic!("expected SessionSwitch, got {other:?}"),
    }

    let loaded = loaded_session_id.lock().expect("lock");
    assert_eq!(loaded.as_deref(), Some("test-session-123"));
}

#[tokio::test]
async fn dispatch_ui_request_toggle_mcp() {
    let mut runtime = AgentSwitchRuntime::default();
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::ToggleMcp {
            server: "test-server".to_string(),
            enable: true,
        },
        &mut runtime,
        &None,
        response_tx,
    )
    .await;

    let response = response_rx.recv().await.expect("response should be sent");
    match response {
        UiRequestResponse::McpToggle {
            server,
            result,
            total,
            server_count,
            names_by_server,
        } => {
            assert_eq!(server, "test-server");
            assert_eq!(result, Ok(McpUsabilityState::Disabled));
            assert_eq!(total, 0);
            assert_eq!(server_count, 0);
            assert!(names_by_server.is_empty());
        }
        other => panic!("expected McpToggle, got {other:?}"),
    }
}

#[tokio::test]
async fn dispatch_ui_request_refresh_session_picker() {
    let mut runtime = AgentSwitchRuntime::default();
    let (response_tx, mut response_rx) = tokio::sync::mpsc::channel::<UiRequestResponse>(32);

    CommandRouter::dispatch_ui_request(
        UiRequest::RefreshSessionPicker,
        &mut runtime,
        &None,
        response_tx,
    )
    .await;

    let response = response_rx.recv().await.expect("response should be sent");
    match response {
        UiRequestResponse::SessionRefresh(result) => {
            assert!(result.is_ok(), "default list_sessions returns Ok");
        }
        other => panic!("expected SessionRefresh, got {other:?}"),
    }
}
