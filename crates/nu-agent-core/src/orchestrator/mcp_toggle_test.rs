use super::test_shared::*;

// ── McpToggleRuntime ────────────────────────────────────────────────────

struct McpToggleRuntime {
    toggles: Vec<(String, bool)>,
    next_state: McpUsabilityState,
    visible_count: usize,
    visible_count_by_server: usize,
}

impl CoreRuntime for McpToggleRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        Ok(Value::nothing(span))
    }
}

impl HasMcpManagement for McpToggleRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        self.toggles.push((server_name.to_string(), enabled));
        Ok(self.next_state)
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.visible_count
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        self.visible_count_by_server
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        Vec::new()
    }
}

impl HasModelSwitching for McpToggleRuntime {
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

crate::default_session!(McpToggleRuntime);
crate::default_compaction!(McpToggleRuntime);

// ── FailingMcpToggleRuntime ─────────────────────────────────────────────

struct FailingMcpToggleRuntime {
    toggles: Vec<(String, bool)>,
    visible_count: usize,
}

impl CoreRuntime for FailingMcpToggleRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        Ok(Value::nothing(span))
    }
}

impl HasMcpManagement for FailingMcpToggleRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        self.toggles.push((server_name.to_string(), enabled));
        Err("connect timeout".to_string())
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.visible_count
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        0
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        Vec::new()
    }
}

impl HasModelSwitching for FailingMcpToggleRuntime {
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

crate::default_session!(FailingMcpToggleRuntime);
crate::default_compaction!(FailingMcpToggleRuntime);

// ── SequencedMcpToggleRuntime ───────────────────────────────────────────

struct SequencedMcpToggleRuntime {
    toggles: Vec<(String, bool)>,
    states: std::collections::VecDeque<McpUsabilityState>,
    visible_counts: std::collections::VecDeque<usize>,
    visible_counts_by_server: std::collections::VecDeque<usize>,
    current_visible_count: usize,
    current_visible_count_by_server: usize,
}

impl CoreRuntime for SequencedMcpToggleRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        Ok(Value::nothing(span))
    }
}

impl HasMcpManagement for SequencedMcpToggleRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        server_name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        self.toggles.push((server_name.to_string(), enabled));
        self.current_visible_count = self
            .visible_counts
            .pop_front()
            .expect("global visible count entry");
        self.current_visible_count_by_server = self
            .visible_counts_by_server
            .pop_front()
            .expect("per-server visible count entry");
        Ok(self.states.pop_front().expect("state sequence entry"))
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.current_visible_count
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        self.current_visible_count_by_server
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        Vec::new()
    }
}

impl HasModelSwitching for SequencedMcpToggleRuntime {
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

crate::default_session!(SequencedMcpToggleRuntime);
crate::default_compaction!(SequencedMcpToggleRuntime);

// ── PanicOnToggleRuntime ────────────────────────────────────────────────

struct PanicOnToggleRuntime {
    visible_count: usize,
}

impl CoreRuntime for PanicOnToggleRuntime {
    fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        Ok(Value::nothing(span))
    }
}

impl HasMcpManagement for PanicOnToggleRuntime {
    fn set_mcp_server_enabled(
        &mut self,
        _name: &str,
        _enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        panic!("toggle panic")
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.visible_count
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        0
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        Vec::new()
    }
}

impl HasModelSwitching for PanicOnToggleRuntime {
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

crate::default_session!(PanicOnToggleRuntime);
crate::default_compaction!(PanicOnToggleRuntime);

// ── StagedToggleUi ──────────────────────────────────────────────────────

struct StagedToggleUi {
    quit: bool,
    first_sent: bool,
    second_sent: bool,
    mcp_states: Vec<(String, McpUsabilityState)>,
    mcp_details: Vec<(String, McpUsabilityState, Option<String>, usize)>,
    mcp_visible_tool_count_updates: Vec<(String, usize)>,
}

impl StagedToggleUi {
    fn new() -> Self {
        Self {
            quit: false,
            first_sent: false,
            second_sent: false,
            mcp_states: Vec::new(),
            mcp_details: Vec::new(),
            mcp_visible_tool_count_updates: Vec::new(),
        }
    }
}

impl ProgressUi for StagedToggleUi {
    fn emit(&mut self, _event: &UiEvent) {}

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl LifecycleUi for StagedToggleUi {
    fn pump_once(&mut self) {
        if self.mcp_details.len() >= 2 {
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

impl UserInputUi for StagedToggleUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        None
    }

    fn take_next_mcp_toggle_request(&mut self) -> Option<McpToggleRequest> {
        if !self.first_sent {
            self.first_sent = true;
            return Some(McpToggleRequest {
                server_name: "gh".to_string(),
                enable: false,
            });
        }

        if self.first_sent && !self.second_sent && !self.mcp_details.is_empty() {
            self.second_sent = true;
            return Some(McpToggleRequest {
                server_name: "gh".to_string(),
                enable: true,
            });
        }

        None
    }
}

impl DisplayStateUi for StagedToggleUi {
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

    fn execute_shared_ui_action(&mut self, _action: SharedUiAction) -> bool {
        true
    }
}

impl TranscriptUi for StagedToggleUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
    }
}

#[test]
fn interactive_loop_processes_mcp_toggle_requests_and_updates_ui_state() {
    let mut runtime = McpToggleRuntime {
        toggles: Vec::new(),
        next_state: McpUsabilityState::Disabled,
        visible_count: 3,
        visible_count_by_server: 0,
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]).with_expected_mcp_updates(1);
    ui.mcp_toggle_requests.push_back(McpToggleRequest {
        server_name: "gh".to_string(),
        enable: false,
    });

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), false)]);
    assert_eq!(
        ui.mcp_states,
        vec![("gh".to_string(), McpUsabilityState::Disabled)]
    );
    assert_eq!(
        ui.mcp_details,
        vec![("gh".to_string(), McpUsabilityState::Disabled, None, 3)]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates,
        vec![("gh".to_string(), 0)]
    );
}

#[test]
fn interactive_loop_marks_enable_failure_as_failed_state() {
    let mut runtime = McpToggleRuntime {
        toggles: Vec::new(),
        next_state: McpUsabilityState::Failed,
        visible_count: 2,
        visible_count_by_server: 0,
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]).with_expected_mcp_updates(1);
    ui.mcp_toggle_requests.push_back(McpToggleRequest {
        server_name: "gh".to_string(),
        enable: true,
    });

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), true)]);
    assert_eq!(
        ui.mcp_states,
        vec![("gh".to_string(), McpUsabilityState::Failed)]
    );
    assert_eq!(
        ui.mcp_details,
        vec![("gh".to_string(), McpUsabilityState::Failed, None, 2)]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates,
        vec![("gh".to_string(), 0)]
    );
}

#[test]
fn interactive_loop_marks_enable_success_as_enabled_state() {
    let mut runtime = McpToggleRuntime {
        toggles: Vec::new(),
        next_state: McpUsabilityState::Enabled,
        visible_count: 7,
        visible_count_by_server: 5,
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]).with_expected_mcp_updates(1);
    ui.mcp_toggle_requests.push_back(McpToggleRequest {
        server_name: "gh".to_string(),
        enable: true,
    });

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), true)]);
    assert_eq!(
        ui.mcp_states,
        vec![("gh".to_string(), McpUsabilityState::Enabled)]
    );
    assert_eq!(
        ui.mcp_details,
        vec![("gh".to_string(), McpUsabilityState::Enabled, None, 7)]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates,
        vec![("gh".to_string(), 5)]
    );
}

#[test]
fn interactive_loop_propagates_failure_reason_and_visible_tool_count_on_toggle_error() {
    let mut runtime = FailingMcpToggleRuntime {
        toggles: Vec::new(),
        visible_count: 4,
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]).with_expected_mcp_updates(1);
    ui.mcp_toggle_requests.push_back(McpToggleRequest {
        server_name: "gh".to_string(),
        enable: true,
    });

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), true)]);
    assert_eq!(
        ui.mcp_states,
        vec![("gh".to_string(), McpUsabilityState::Failed)]
    );
    assert_eq!(
        ui.mcp_details,
        vec![(
            "gh".to_string(),
            McpUsabilityState::Failed,
            Some("connect timeout".to_string()),
            4,
        )]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates,
        vec![("gh".to_string(), 0)]
    );
}

#[test]
fn interactive_toggle_enable_disable_cycle_refreshes_per_server_visible_counts() {
    let mut runtime = SequencedMcpToggleRuntime {
        toggles: Vec::new(),
        states: [McpUsabilityState::Disabled, McpUsabilityState::Enabled]
            .into_iter()
            .collect(),
        visible_counts: [3usize, 7usize].into_iter().collect(),
        visible_counts_by_server: [0usize, 5usize].into_iter().collect(),
        current_visible_count: 0,
        current_visible_count_by_server: 0,
    };
    let mut ui = StagedToggleUi::new();

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.toggles,
        vec![("gh".to_string(), false), ("gh".to_string(), true)]
    );
    assert_eq!(
        ui.mcp_details,
        vec![
            ("gh".to_string(), McpUsabilityState::Disabled, None, 3),
            ("gh".to_string(), McpUsabilityState::Enabled, None, 7),
        ]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates,
        vec![("gh".to_string(), 0), ("gh".to_string(), 5)]
    );
}

#[test]
fn interactive_loop_disconnected_toggle_worker_preserves_authoritative_visible_tool_count() {
    let mut runtime = PanicOnToggleRuntime { visible_count: 9 };
    let mut ui = FakeInteractiveUi::with_prompts(&[]).with_expected_mcp_updates(1);
    ui.mcp_toggle_requests.push_back(McpToggleRequest {
        server_name: "gh".to_string(),
        enable: false,
    });

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None);
    }));

    assert!(panic.is_err(), "expected panic from toggle worker thread");
    assert_eq!(
        ui.mcp_details,
        vec![(
            "gh".to_string(),
            McpUsabilityState::Failed,
            Some("toggle worker disconnected".to_string()),
            9,
        )]
    );
}

#[test]
fn interactive_loop_worker_channel_closed_preserves_authoritative_visible_tool_count() {
    let mut runtime = PanicOnToggleRuntime { visible_count: 13 };
    let mut ui = StagedToggleUi::new();

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None);
    }));

    assert!(panic.is_err(), "expected panic from toggle worker thread");
    assert_eq!(
        ui.mcp_details.len(),
        2,
        "expected exactly two toggle failure reports"
    );
    // First toggle: worker panics, response channel disconnects
    assert_eq!(ui.mcp_details[0].0, "gh");
    assert_eq!(ui.mcp_details[0].1, McpUsabilityState::Failed);
    assert_eq!(
        ui.mcp_details[0].2.as_deref(),
        Some("toggle worker disconnected")
    );
    assert_eq!(ui.mcp_details[0].3, 13);
    // Second toggle: either the command channel is already closed ("worker channel closed")
    // or the send succeeds but the response channel is disconnected ("toggle worker disconnected").
    // Both are valid — the exact outcome depends on thread scheduling.
    assert_eq!(ui.mcp_details[1].0, "gh");
    assert_eq!(ui.mcp_details[1].1, McpUsabilityState::Failed);
    let reason = ui.mcp_details[1].2.as_deref().unwrap_or("");
    assert!(
        reason == "worker channel closed" || reason == "toggle worker disconnected",
        "unexpected reason for second toggle: {reason}"
    );
    assert_eq!(ui.mcp_details[1].3, 13);
    assert!(ui.mcp_visible_tool_count_updates.is_empty());
}
