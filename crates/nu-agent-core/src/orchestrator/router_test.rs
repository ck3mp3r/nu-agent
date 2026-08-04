// Tests for CommandRouter agent-switch dispatch through run_interactive_loop.
//
// These verify that SwitchAgent commands dispatched by CommandRouter on the
// worker thread produce the correct UI updates when the orchestrator loop
// processes the response channel.

use nu_protocol::{LabeledError, Span, Value};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use crate::orchestrator::{
    CommandRouter, InteractiveLoopConfig, OnAgentSwitch, WorkerCommand, run_interactive_loop_impl,
};
use crate::protocol::{
    compaction::{CompactionTriggerDecision, CompactionTriggerSource},
    compaction_runtime::Compaction,
    contracts::{
        CoreRuntime, DisplayStateUi, LifecycleUi, McpUsabilityState, ProgressUi, SharedUiAction,
        TranscriptUi, UiMessageSnapshot, UserInputUi,
    },
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
    submitted: std::collections::VecDeque<String>,
    agent_switch_requests: std::collections::VecDeque<String>,
    quit: bool,
    pump_count: usize,
    warnings: Vec<String>,
    active_model_identity: Option<String>,
    active_agent_identity: Option<String>,
}

impl AgentSwitchUi {
    fn new(agent_switches: &[&str]) -> Self {
        Self {
            submitted: std::collections::VecDeque::new(),
            agent_switch_requests: agent_switches.iter().map(|s| s.to_string()).collect(),
            quit: false,
            pump_count: 0,
            warnings: Vec::new(),
            active_model_identity: None,
            active_agent_identity: None,
        }
    }
}

impl ProgressUi for AgentSwitchUi {
    fn emit(&mut self, event: &UiEvent) {
        if let UiEvent::Warning { message } = event {
            self.warnings.push(message.clone());
        }
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl LifecycleUi for AgentSwitchUi {
    fn pump_once(&mut self) {
        self.pump_count = self.pump_count.saturating_add(1);
        if self.agent_switch_requests.is_empty() && self.pump_count > 1 {
            self.quit = true;
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        None
    }
}

impl UserInputUi for AgentSwitchUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn take_next_agent_switch_request(&mut self) -> Option<String> {
        self.agent_switch_requests.pop_front()
    }
}

impl DisplayStateUi for AgentSwitchUi {
    fn set_active_model_identity(&mut self, active_model_identity: &str) {
        self.active_model_identity = Some(active_model_identity.to_string());
    }

    fn set_active_agent_identity(&mut self, name: &str) {
        self.active_agent_identity = Some(name.to_string());
    }

    fn set_mcp_server_state(&mut self, _server_name: &str, _state: McpUsabilityState) {}

    fn execute_shared_ui_action(&mut self, _action: SharedUiAction) -> bool {
        true
    }
}

impl TranscriptUi for AgentSwitchUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
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
    submitted: std::collections::VecDeque<String>,
    injected_agent_switch_during_active: Vec<String>,
    agent_switch_requests: std::collections::VecDeque<String>,
    injected: bool,
    quit: bool,
    pump_count: usize,
    active: Arc<AtomicBool>,
    block_first_turn: Arc<AtomicBool>,
    completed_count: usize,
    expected_completions: usize,
    active_pump_count: Arc<AtomicUsize>,
    warnings: Vec<String>,
    active_model_identity: Option<String>,
    active_agent_identity: Option<String>,
}

impl ResponsiveAgentSwitchUi {
    fn new(
        initial_prompts: &[&str],
        injected_agent_switch_during_active: &[&str],
        active: Arc<AtomicBool>,
        block_first_turn: Arc<AtomicBool>,
        expected_completions: usize,
        active_pump_count: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            submitted: initial_prompts.iter().map(|s| s.to_string()).collect(),
            injected_agent_switch_during_active: injected_agent_switch_during_active
                .iter()
                .map(|s| s.to_string())
                .collect(),
            agent_switch_requests: std::collections::VecDeque::new(),
            injected: false,
            quit: false,
            pump_count: 0,
            active,
            block_first_turn,
            completed_count: 0,
            expected_completions,
            active_pump_count,
            warnings: Vec::new(),
            active_model_identity: None,
            active_agent_identity: None,
        }
    }
}

impl ProgressUi for ResponsiveAgentSwitchUi {
    fn emit(&mut self, event: &UiEvent) {
        if let UiEvent::Completed { .. } = event {
            self.completed_count += 1;
            if self.completed_count >= self.expected_completions {
                self.quit = true;
            }
        } else if let UiEvent::Warning { message } = event {
            self.warnings.push(message.clone());
        }
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl LifecycleUi for ResponsiveAgentSwitchUi {
    fn pump_once(&mut self) {
        self.pump_count += 1;
        if self.active.load(Ordering::SeqCst) {
            self.active_pump_count.fetch_add(1, Ordering::SeqCst);
            if !self.injected {
                for request in self.injected_agent_switch_during_active.clone() {
                    self.agent_switch_requests.push_back(request);
                }
                self.injected = true;
                self.block_first_turn.store(true, Ordering::SeqCst);
            }
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        None
    }
}

impl UserInputUi for ResponsiveAgentSwitchUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn take_next_agent_switch_request(&mut self) -> Option<String> {
        self.agent_switch_requests.pop_front()
    }
}

impl DisplayStateUi for ResponsiveAgentSwitchUi {
    fn set_active_model_identity(&mut self, active_model_identity: &str) {
        self.active_model_identity = Some(active_model_identity.to_string());
    }

    fn set_active_agent_identity(&mut self, name: &str) {
        self.active_agent_identity = Some(name.to_string());
    }

    fn set_mcp_server_state(&mut self, _server_name: &str, _state: McpUsabilityState) {}

    fn execute_shared_ui_action(&mut self, _action: SharedUiAction) -> bool {
        true
    }
}

impl TranscriptUi for ResponsiveAgentSwitchUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_switch_sends_command_and_updates_ui_identity() {
    let runtime = AgentSwitchRuntime::default();
    let mut ui = AgentSwitchUi::new(&["research-agent"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.switched_agents, vec!["research-agent"]);
    assert_eq!(ui.active_agent_identity, Some("research-agent".to_string()));
    assert_eq!(
        ui.active_model_identity,
        Some("openai/gpt-4o-mini".to_string())
    );
}

#[tokio::test]
async fn agent_switch_failure_warns_and_keeps_previous_agent() {
    let runtime = AgentSwitchRuntime {
        switch_agent_result: Some(Err("agent not found".to_string())),
        ..Default::default()
    };
    let mut ui = AgentSwitchUi::new(&["nonexistent-agent"]);

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert!(ui.warnings.iter().any(|w| w == "agent not found"));
    assert_eq!(ui.active_agent_identity, None);
}

#[tokio::test]
async fn agent_switch_while_worker_active_is_queued_for_next_turn() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningAgentRuntime::new(Arc::clone(&block_first_turn));
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let mut ui = ResponsiveAgentSwitchUi::new(
        &["first"],
        &["research-agent"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        1,
        Arc::clone(&active_pump_count),
    );

    // Spawn a background task to unblock the turn after a delay.
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        unblock.store(true, Ordering::SeqCst);
    });

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
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
            .iter()
            .any(|w| w == "Agent switch queued for next turn: research-agent")
    );
    assert_eq!(ui.active_agent_identity, Some("research-agent".to_string()));
}

#[tokio::test]
async fn queued_agent_switch_last_write_wins() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningAgentRuntime::new(Arc::clone(&block_first_turn));
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let mut ui = ResponsiveAgentSwitchUi::new(
        &["first"],
        &["agent-a", "agent-b"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        1,
        Arc::clone(&active_pump_count),
    );

    // Spawn a background task to unblock the turn after a delay.
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        unblock.store(true, Ordering::SeqCst);
    });

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
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
    assert_eq!(ui.active_agent_identity, Some("agent-b".to_string()));
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
        _ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
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
    let (response_tx, response_rx) = std::sync::mpsc::channel();

    let cmd = WorkerCommand::SwitchSession {
        session_id: "test-session-123".to_string(),
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
    let loaded = loaded_session_id.lock().expect("lock");
    assert_eq!(
        loaded.as_deref(),
        Some("test-session-123"),
        "load_session should be called with the correct session_id"
    );
    drop(loaded);

    // 3. The result is sent through the response channel
    let result = response_rx.recv().expect("response should be sent");
    assert!(
        result.is_err(),
        "default load_session should return an error"
    );
}

// ---------------------------------------------------------------------------
// on_agent_switch callback
// ---------------------------------------------------------------------------

#[tokio::test]
async fn on_agent_switch_callback_invoked_after_successful_switch() {
    let switched_name = Arc::new(std::sync::Mutex::new(None::<String>));
    let switched_desc = Arc::new(std::sync::Mutex::new(None::<Option<String>>));
    let cb_name = Arc::clone(&switched_name);
    let cb_desc = Arc::clone(&switched_desc);

    let callback: OnAgentSwitch = Arc::new(move |name: String, description: Option<String>| {
        *cb_name.lock().expect("name lock") = Some(name);
        *cb_desc.lock().expect("desc lock") = Some(description);
    });

    let runtime = AgentSwitchRuntime {
        switch_agent_result: Some(Ok("research-agent".to_string())),
        ..Default::default()
    };
    let mut ui = AgentSwitchUi::new(&["research-agent"]);

    let config = InteractiveLoopConfig::new(Span::test_data()).with_on_agent_switch(callback);

    let (_runtime, result) = run_interactive_loop_impl(runtime, &mut ui, config).await;
    let value = result.expect("interactive loop");
    assert!(value.is_nothing());

    // Verify the callback was invoked with the correct values
    let name = switched_name.lock().expect("name lock").take();
    let desc = switched_desc.lock().expect("desc lock").take();
    assert_eq!(name.as_deref(), Some("research-agent"));
    // The AgentSwitchRuntime doesn't implement agent_description(), so it
    // returns None (the default trait method).
    assert_eq!(desc, Some(None));
}
