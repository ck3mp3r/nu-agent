pub(crate) use nu_protocol::{LabeledError, Span, Value};
pub(crate) use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
pub(crate) use std::time::Duration;

pub(crate) use crate::compaction::CompactionStrategy;
pub(crate) use crate::orchestrator::{
    run_hydrated_interactive_loop, run_interactive_loop, run_single_turn,
};
pub(crate) use crate::protocol::{
    compaction::{CompactionTriggerDecision, CompactionTriggerSource},
    compaction_runtime::HasCompaction,
    contracts::{
        CoreRuntime, DisplayStateUi, LifecycleUi, McpToggleRequest, McpUsabilityState, ProgressUi,
        SharedUiAction, TranscriptUi, UiMessageSnapshot, UiMessageUsageSnapshot, UserInputUi,
    },
    event::{
        PermissionDecision, PermissionDecisionSubmission, PermissionRequestContext, ToolDisplay,
        ToolDisplaySection, UiEvent,
    },
    mcp_management::HasMcpManagement,
    model_switching::HasModelSwitching,
    session_management::HasSessionManagement,
};

// Convenience: empty impl blocks that use the trait's default methods.
// Since the protocol traits now provide defaults, test runtimes that don't
// need custom behavior only need these empty blocks to satisfy trait bounds.
#[macro_export]
macro_rules! default_session {
    ($t:ty) => {
        impl HasSessionManagement for $t {}
    };
}
#[macro_export]
macro_rules! default_mcp {
    ($t:ty) => {
        impl HasMcpManagement for $t {}
    };
}
#[macro_export]
macro_rules! default_compaction {
    ($t:ty) => {
        impl HasCompaction for $t {}
    };
}

pub(crate) struct FakeInteractiveUi {
    pub(crate) submitted: std::collections::VecDeque<String>,
    pub(crate) quit: bool,
    pub(crate) pump_count: usize,
    pub(crate) min_pump_count: usize,
    pub(crate) call_order: Vec<&'static str>,
    pub(crate) hydrated_messages: Vec<UiMessageSnapshot>,
    pub(crate) mcp_toggle_requests: std::collections::VecDeque<McpToggleRequest>,
    pub(crate) mcp_states: Vec<(String, McpUsabilityState)>,
    pub(crate) mcp_details: Vec<(String, McpUsabilityState, Option<String>, usize)>,
    pub(crate) mcp_visible_tool_count_updates: Vec<(String, usize)>,
    pub(crate) warnings: Vec<String>,
    pub(crate) expected_mcp_updates: usize,
    pub(crate) shared_actions: Vec<SharedUiAction>,
    pub(crate) model_switch_requests: std::collections::VecDeque<String>,
    pub(crate) active_model_identity: Option<String>,
    pub(crate) agent_switch_requests: std::collections::VecDeque<String>,
    pub(crate) active_agent_identity: Option<String>,
}

impl FakeInteractiveUi {
    pub(crate) fn with_prompts(prompts: &[&str]) -> Self {
        Self {
            submitted: prompts.iter().map(|s| s.to_string()).collect(),
            quit: false,
            pump_count: 0,
            min_pump_count: 1,
            call_order: Vec::new(),
            hydrated_messages: Vec::new(),
            mcp_toggle_requests: std::collections::VecDeque::new(),
            mcp_states: Vec::new(),
            mcp_details: Vec::new(),
            mcp_visible_tool_count_updates: Vec::new(),
            warnings: Vec::new(),
            expected_mcp_updates: 0,
            shared_actions: Vec::new(),
            model_switch_requests: std::collections::VecDeque::new(),
            active_model_identity: None,
            agent_switch_requests: std::collections::VecDeque::new(),
            active_agent_identity: None,
        }
    }

    pub(crate) fn with_expected_mcp_updates(mut self, expected_mcp_updates: usize) -> Self {
        self.expected_mcp_updates = expected_mcp_updates;
        self
    }

    pub(crate) fn with_min_pump_count(mut self, min_pump_count: usize) -> Self {
        self.min_pump_count = min_pump_count;
        self
    }
}

impl ProgressUi for FakeInteractiveUi {
    fn emit(&mut self, event: &UiEvent) {
        if let UiEvent::Warning { message } = event {
            self.warnings.push(message.clone());
        }
        if let UiEvent::TurnError { message } = event {
            self.warnings.push(message.clone());
        }
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl LifecycleUi for FakeInteractiveUi {
    fn pump_once(&mut self) {
        self.pump_count = self.pump_count.saturating_add(1);
        self.call_order.push("pump_once");
        if self.submitted.is_empty()
            && self.mcp_toggle_requests.is_empty()
            && self.mcp_states.len() >= self.expected_mcp_updates
            && self.pump_count > self.min_pump_count
        {
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

impl UserInputUi for FakeInteractiveUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn take_next_mcp_toggle_request(&mut self) -> Option<McpToggleRequest> {
        self.mcp_toggle_requests.pop_front()
    }

    fn take_next_model_switch_request(&mut self) -> Option<String> {
        self.model_switch_requests.pop_front()
    }

    fn take_next_agent_picker_launch_request(&mut self) -> bool {
        false
    }

    fn take_next_agent_switch_request(&mut self) -> Option<String> {
        self.agent_switch_requests.pop_front()
    }
}

impl DisplayStateUi for FakeInteractiveUi {
    fn set_mcp_server_state(&mut self, server_name: &str, state: McpUsabilityState) {
        self.mcp_states.push((server_name.to_string(), state));
    }

    fn set_mcp_server_state_with_details(
        &mut self,
        server_name: &str,
        state: McpUsabilityState,
        reason: Option<String>,
        llm_visible_mcp_tool_count: usize,
    ) {
        self.mcp_details.push((
            server_name.to_string(),
            state,
            reason,
            llm_visible_mcp_tool_count,
        ));
        self.set_mcp_server_state(server_name, state);
    }

    fn set_mcp_visible_tool_count_by_server_name(&mut self, server_name: &str, count: usize) {
        self.mcp_visible_tool_count_updates
            .push((server_name.to_string(), count));
    }

    fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        _server_name: &str,
        _names: Vec<String>,
    ) {
    }

    fn execute_shared_ui_action(&mut self, action: SharedUiAction) -> bool {
        self.shared_actions.push(action);
        true
    }

    fn set_active_model_identity(&mut self, active_model_identity: &str) {
        self.active_model_identity = Some(active_model_identity.to_string());
    }

    fn set_active_agent_identity(&mut self, name: &str) {
        self.active_agent_identity = Some(name.to_string());
    }
}

impl TranscriptUi for FakeInteractiveUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
        self.call_order.push("hydrate");
        self.hydrated_messages.extend(messages);
    }
}

#[derive(Default)]
pub(crate) struct FakeRuntime {
    pub(crate) prompts: Vec<String>,
    pub(crate) auto_decisions: std::collections::VecDeque<CompactionTriggerDecision>,
    pub(crate) executed_compaction_sources: Vec<CompactionTriggerSource>,
    pub(crate) fail_compaction: bool,
    pub(crate) compaction_call_count: usize,
    pub(crate) switched_models: Vec<String>,
    pub(crate) switch_model_result: Option<Result<String, String>>,
}

impl CoreRuntime for FakeRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        Ok(Value::nothing(Span::test_data()))
    }
}

impl HasMcpManagement for FakeRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        _name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(if enabled {
            McpUsabilityState::Enabled
        } else {
            McpUsabilityState::Disabled
        })
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

impl HasModelSwitching for FakeRuntime {
    fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String> {
        self.switched_models.push(model_spec.to_string());
        if let Some(result) = self.switch_model_result.clone() {
            return result.map(|identity| (identity, None));
        }
        Ok((model_spec.to_string(), None))
    }

    fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported in this runtime".to_string())
    }

    fn active_model_identity(&self) -> String {
        "openai/gpt-4o-mini".to_string()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

impl HasCompaction for FakeRuntime {
    fn evaluate_auto_compaction(&mut self) -> Option<CompactionTriggerDecision> {
        self.auto_decisions.pop_front()
    }

    fn execute_compaction_trigger<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        source: CompactionTriggerSource,
    ) -> Result<(), String> {
        self.compaction_call_count = self.compaction_call_count.saturating_add(1);
        self.executed_compaction_sources.push(source);
        if self.fail_compaction {
            return Err("auto compaction failed".to_string());
        }
        Ok(())
    }
}

impl HasSessionManagement for FakeRuntime {}

#[derive(Default)]
pub(crate) struct FakeValueRuntime {
    pub(crate) prompts: Vec<String>,
}

impl CoreRuntime for FakeValueRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        Ok(Value::record(nu_protocol::Record::new(), span))
    }
}

impl HasModelSwitching for FakeValueRuntime {
    fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported in this runtime".to_string())
    }

    fn active_model_identity(&self) -> String {
        "unknown/unknown".to_string()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

crate::default_session!(FakeValueRuntime);
crate::default_mcp!(FakeValueRuntime);
crate::default_compaction!(FakeValueRuntime);

#[derive(Default)]
pub(crate) struct LongRunningRuntime {
    pub(crate) prompts: Arc<Mutex<Vec<String>>>,
    pub(crate) switched_models: Arc<Mutex<Vec<String>>>,
    pub(crate) action_log: Arc<Mutex<Vec<String>>>,
    pub(crate) active: Arc<AtomicBool>,
    pub(crate) block_first_turn: Arc<AtomicBool>,
    pub(crate) switch_model_result: Option<Result<String, String>>,
    pub(crate) active_identity: Arc<Mutex<String>>,
}

impl LongRunningRuntime {
    pub(crate) fn new(block_first_turn: Arc<AtomicBool>) -> Self {
        Self {
            prompts: Arc::new(Mutex::new(Vec::new())),
            switched_models: Arc::new(Mutex::new(Vec::new())),
            action_log: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(AtomicBool::new(false)),
            block_first_turn,
            switch_model_result: None,
            active_identity: Arc::new(Mutex::new("openai/gpt-4o-mini".to_string())),
        }
    }

    pub(crate) fn with_switch_model_result(mut self, result: Result<String, String>) -> Self {
        self.switch_model_result = Some(result);
        self
    }
}

impl CoreRuntime for LongRunningRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.active.store(true, Ordering::SeqCst);
        self.action_log
            .lock()
            .expect("action log lock")
            .push(format!("turn:{prompt}"));
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
                std::thread::sleep(Duration::from_millis(2));
            }
        }

        ui.emit(&UiEvent::Completed { tool_calls: 0 });
        self.active.store(false, Ordering::SeqCst);
        Ok(Value::nothing(Span::test_data()))
    }
}

impl HasModelSwitching for LongRunningRuntime {
    fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String> {
        self.action_log
            .lock()
            .expect("action log lock")
            .push(format!("switch:{model_spec}"));
        self.switched_models
            .lock()
            .expect("switched models lock")
            .push(model_spec.to_string());

        if let Some(result) = self.switch_model_result.clone() {
            return result.map(|identity| (identity, None));
        }

        let mut identity = self.active_identity.lock().expect("identity lock");
        *identity = model_spec.to_string();
        Ok((model_spec.to_string(), None))
    }

    fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported in this runtime".to_string())
    }

    fn active_model_identity(&self) -> String {
        self.active_identity.lock().expect("identity lock").clone()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

crate::default_session!(LongRunningRuntime);
crate::default_mcp!(LongRunningRuntime);
crate::default_compaction!(LongRunningRuntime);

pub(crate) struct ResponsiveInteractiveUi {
    pub(crate) submitted: std::collections::VecDeque<String>,
    pub(crate) injected_during_active: Vec<String>,
    pub(crate) injected_model_switch_during_active: Vec<String>,
    pub(crate) model_switch_requests: std::collections::VecDeque<String>,
    pub(crate) injected: bool,
    pub(crate) quit: bool,
    pub(crate) fatal: Option<String>,
    pub(crate) pump_count: usize,
    pub(crate) active: Arc<AtomicBool>,
    pub(crate) block_first_turn: Arc<AtomicBool>,
    pub(crate) completed_count: usize,
    pub(crate) expected_completions: usize,
    pub(crate) active_pump_count: Arc<AtomicUsize>,
    pub(crate) mcp_states: Vec<(String, McpUsabilityState)>,
    pub(crate) warnings: Vec<String>,
}

impl ResponsiveInteractiveUi {
    pub(crate) fn new(
        initial_prompts: &[&str],
        injected_during_active: &[&str],
        injected_model_switch_during_active: &[&str],
        active: Arc<AtomicBool>,
        block_first_turn: Arc<AtomicBool>,
        expected_completions: usize,
        active_pump_count: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            submitted: initial_prompts.iter().map(|s| s.to_string()).collect(),
            injected_during_active: injected_during_active
                .iter()
                .map(|s| s.to_string())
                .collect(),
            injected_model_switch_during_active: injected_model_switch_during_active
                .iter()
                .map(|s| s.to_string())
                .collect(),
            model_switch_requests: std::collections::VecDeque::new(),
            injected: false,
            quit: false,
            fatal: None,
            pump_count: 0,
            active,
            block_first_turn,
            completed_count: 0,
            expected_completions,
            active_pump_count,
            mcp_states: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

impl ProgressUi for ResponsiveInteractiveUi {
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

impl LifecycleUi for ResponsiveInteractiveUi {
    fn pump_once(&mut self) {
        self.pump_count += 1;
        if self.active.load(Ordering::SeqCst) {
            self.active_pump_count.fetch_add(1, Ordering::SeqCst);
            if !self.injected {
                for prompt in self.injected_during_active.clone() {
                    self.submitted.push_back(prompt);
                }
                for request in self.injected_model_switch_during_active.clone() {
                    self.model_switch_requests.push_back(request);
                }
                self.injected = true;
                // Deterministic unblock: once we've injected prompts during active work,
                // allow the first turn to complete so queued prompts can proceed.
                self.block_first_turn.store(true, Ordering::SeqCst);
            }
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        self.fatal.as_deref()
    }
}

impl UserInputUi for ResponsiveInteractiveUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn take_next_model_switch_request(&mut self) -> Option<String> {
        self.model_switch_requests.pop_front()
    }
}

impl DisplayStateUi for ResponsiveInteractiveUi {
    fn set_mcp_server_state(&mut self, server_name: &str, state: McpUsabilityState) {
        self.mcp_states.push((server_name.to_string(), state));
    }

    fn execute_shared_ui_action(&mut self, _action: SharedUiAction) -> bool {
        true
    }
}

impl TranscriptUi for ResponsiveInteractiveUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
    }
}

// Compile-time check: run_single_turn must accept anything that impls CoreRuntime
// This test will fail to compile until CoreRuntime exists and run_single_turn uses it
pub(crate) fn _assert_single_turn_accepts_core_runtime<R: CoreRuntime + Send, U: ProgressUi>(
    _r: R,
    _u: U,
) {
    // if this compiles, the bound is correct
}

// Compile-time check: run_interactive_loop must accept anything that impls the focused capability traits
pub(crate) fn _assert_interactive_loop_accepts_extended_runtime<
    R: CoreRuntime
        + HasMcpManagement
        + HasModelSwitching
        + HasSessionManagement
        + HasCompaction
        + Send,
    U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
>(
    _r: R,
    _u: U,
) {
    // if this compiles, the bound is correct
}
