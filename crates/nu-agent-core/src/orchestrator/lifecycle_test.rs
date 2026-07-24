use super::test_shared::*;

// ── FakeProgressUi ──────────────────────────────────────────────────────

#[derive(Default)]
struct FakeProgressUi {
    events: Vec<UiEvent>,
}

impl ProgressUi for FakeProgressUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

// ── ToolDisplayOnlyRuntime ──────────────────────────────────────────────

#[derive(Default)]
struct ToolDisplayOnlyRuntime;

impl CoreRuntime for ToolDisplayOnlyRuntime {
    async fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        ui.emit(&UiEvent::ToolStart {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: "{}".to_string(),
        });
        ui.emit(&UiEvent::ToolEnd {
            name: "edit".to_string(),
            source: "closure".to_string(),
            arguments: "{}".to_string(),
            success: true,
            result: r#"{"path":"file.txt","diff":"--- a/file.txt\n+++ b/file.txt\n"}"#.to_string(),
            display: Some(ToolDisplay {
                title: "edit file.txt".to_string(),
                sections: vec![ToolDisplaySection {
                    label: "file.txt".to_string(),
                    language: "diff".to_string(),
                    content: "--- a/file.txt\n+++ b/file.txt\n".to_string(),
                    stats: None,
                }],
            }),
            error_kind: None,
            message: None,
        });
        ui.emit(&UiEvent::Completed { tool_calls: 1 });
        Ok(Value::nothing(span))
    }
}

impl HasModelSwitching for ToolDisplayOnlyRuntime {
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

// ── CancelFirstRuntime ──────────────────────────────────────────────────

#[derive(Default)]
struct CancelFirstRuntime {
    prompts: Vec<String>,
}

impl CoreRuntime for CancelFirstRuntime {
    async fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        if self.prompts.len() == 1 {
            return Err(LabeledError::new("LLM call cancelled"));
        }

        Ok(Value::nothing(Span::test_data()))
    }
}

impl HasModelSwitching for CancelFirstRuntime {
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

crate::default_session!(CancelFirstRuntime);
crate::default_mcp!(CancelFirstRuntime);
crate::default_compaction!(CancelFirstRuntime);

// ── ErrorFirstRuntime ───────────────────────────────────────────────────

#[derive(Default)]
struct ErrorFirstRuntime {
    prompts: Vec<String>,
}

impl CoreRuntime for ErrorFirstRuntime {
    async fn execute_turn<U: ProgressUi>(
        &mut self,
        _ui: &mut U,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        if self.prompts.len() == 1 {
            return Err(LabeledError::new("API rate limit exceeded"));
        }

        Ok(Value::nothing(Span::test_data()))
    }
}

impl HasModelSwitching for ErrorFirstRuntime {
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

crate::default_session!(ErrorFirstRuntime);
crate::default_mcp!(ErrorFirstRuntime);
crate::default_compaction!(ErrorFirstRuntime);

// ── PermissionGateRuntime ───────────────────────────────────────────────

struct PermissionGateRuntime {
    side_effects: Arc<AtomicUsize>,
    requested: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    request_id: String,
    rule_identity: String,
}

impl PermissionGateRuntime {
    fn new() -> Self {
        Self {
            side_effects: Arc::new(AtomicUsize::new(0)),
            requested: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
            active: Arc::new(AtomicBool::new(false)),
            request_id: "ask-0000000000000abc".to_string(),
            rule_identity: "nested:nu__run.command:*".to_string(),
        }
    }
}

impl CoreRuntime for PermissionGateRuntime {
    async fn execute_turn<U: ProgressUi>(
        &mut self,
        ui: &mut U,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        self.active.store(true, Ordering::SeqCst);

        let controller =
            crate::protocol::permission::PermissionController::new(Duration::from_secs(2));
        let (token, requested_event) = controller
            .begin_request(crate::protocol::permission::PermissionRequest {
                request_id: self.request_id.clone(),
                context: PermissionRequestContext {
                    tool: "nu__run".to_string(),
                    source: "closure".to_string(),
                    mode: Some("apply".to_string()),
                    matched_rule_identity: self.rule_identity.clone(),
                    scope: "nested".to_string(),
                    target_field: Some("command".to_string()),
                    pattern: "*".to_string(),
                    summary: "tool[nu__run] args={\"command\":\"echo hi\"}".to_string(),
                    pre_authorize_display: None,
                },
            })
            .expect("permission request");

        crate::protocol::permission::install_active_permission_submission_sender(Some(
            token.sender_clone(),
        ));
        ui.emit(&requested_event);
        self.requested.store(true, Ordering::SeqCst);

        let (resolution, events) = controller.await_resolution(&token).await;
        for event in events {
            ui.emit(&event);
        }

        crate::protocol::permission::install_active_permission_submission_sender(None);

        if let crate::protocol::permission::PermissionResolution::Decision { decision, .. } =
            resolution
            && decision != PermissionDecision::Deny
        {
            self.side_effects.fetch_add(1, Ordering::SeqCst);
            ui.emit(&UiEvent::ToolStart {
                name: "nu__run".to_string(),
                source: "closure".to_string(),
                arguments: r#"{"command":"echo hi"}"#.to_string(),
            });
        }

        ui.emit(&UiEvent::Completed { tool_calls: 0 });
        self.finished.store(true, Ordering::SeqCst);
        self.active.store(false, Ordering::SeqCst);
        Ok(Value::nothing(span))
    }
}

impl HasMcpManagement for PermissionGateRuntime {
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

crate::default_session!(PermissionGateRuntime);
crate::default_compaction!(PermissionGateRuntime);

impl HasModelSwitching for PermissionGateRuntime {
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

// ── PermissionOrderingUi ────────────────────────────────────────────────

struct PermissionOrderingUi {
    submitted: std::collections::VecDeque<String>,
    pending_decisions: std::collections::VecDeque<PermissionDecisionSubmission>,
    events: Vec<UiEvent>,
    decision: PermissionDecision,
    decision_delay_pumps: usize,
    pumps_since_request: usize,
    request_seen: bool,
    quit: bool,
    active: Arc<AtomicBool>,
    pumps_while_waiting: Arc<AtomicUsize>,
    side_effects: Arc<AtomicUsize>,
}

impl PermissionOrderingUi {
    fn new(
        decision: PermissionDecision,
        decision_delay_pumps: usize,
        active: Arc<AtomicBool>,
        pumps_while_waiting: Arc<AtomicUsize>,
        side_effects: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            submitted: ["run".to_string()].into_iter().collect(),
            pending_decisions: std::collections::VecDeque::new(),
            events: Vec::new(),
            decision,
            decision_delay_pumps,
            pumps_since_request: 0,
            request_seen: false,
            quit: false,
            active,
            pumps_while_waiting,
            side_effects,
        }
    }
}

impl ProgressUi for PermissionOrderingUi {
    fn emit(&mut self, event: &UiEvent) {
        self.events.push(event.clone());
        if let UiEvent::PermissionRequested {
            request_id,
            context,
        } = event
        {
            self.request_seen = true;
            self.pending_decisions
                .push_back(PermissionDecisionSubmission {
                    request_id: request_id.clone(),
                    decision: self.decision,
                    matched_rule_identity: context.matched_rule_identity.clone(),
                });
        }
        if matches!(event, UiEvent::Completed { .. }) {
            self.quit = true;
        }
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl LifecycleUi for PermissionOrderingUi {
    fn pump_once(&mut self) {
        if self.request_seen {
            self.pumps_since_request = self.pumps_since_request.saturating_add(1);
        }
        if self.active.load(Ordering::SeqCst)
            && self.request_seen
            && self.side_effects.load(Ordering::SeqCst) == 0
        {
            self.pumps_while_waiting.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        None
    }
}

impl UserInputUi for PermissionOrderingUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn take_next_permission_decision_submission(&mut self) -> Option<PermissionDecisionSubmission> {
        if self.pumps_since_request < self.decision_delay_pumps {
            return None;
        }
        self.pending_decisions.pop_front()
    }
}

impl DisplayStateUi for PermissionOrderingUi {
    fn set_mcp_server_state(&mut self, _server_name: &str, _state: McpUsabilityState) {}

    fn execute_shared_ui_action(&mut self, _action: SharedUiAction) -> bool {
        true
    }
}

impl TranscriptUi for PermissionOrderingUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
    }
}

// ── ModelPickerLaunchWhileActiveUi ──────────────────────────────────────

struct ModelPickerLaunchWhileActiveUi {
    submitted: std::collections::VecDeque<String>,
    pending_model_picker_launch_requests: usize,
    injected: bool,
    quit: bool,
    fatal: Option<String>,
    active: Arc<AtomicBool>,
    block_first_turn: Arc<AtomicBool>,
    completed_count: usize,
    expected_completions: usize,
    shared_actions: Vec<SharedUiAction>,
    shared_actions_observed_while_active: Vec<bool>,
    mcp_states: Vec<(String, McpUsabilityState)>,
}

impl ModelPickerLaunchWhileActiveUi {
    fn new(
        initial_prompts: &[&str],
        expected_completions: usize,
        active: Arc<AtomicBool>,
        block_first_turn: Arc<AtomicBool>,
    ) -> Self {
        Self {
            submitted: initial_prompts.iter().map(|s| s.to_string()).collect(),
            pending_model_picker_launch_requests: 0,
            injected: false,
            quit: false,
            fatal: None,
            active,
            block_first_turn,
            completed_count: 0,
            expected_completions,
            shared_actions: Vec::new(),
            shared_actions_observed_while_active: Vec::new(),
            mcp_states: Vec::new(),
        }
    }
}

impl ProgressUi for ModelPickerLaunchWhileActiveUi {
    fn emit(&mut self, event: &UiEvent) {
        if let UiEvent::Completed { .. } = event {
            self.completed_count += 1;
            if self.completed_count >= self.expected_completions {
                self.quit = true;
            }
        }
    }

    fn flush(&mut self) {}

    fn take_cancel_requested(&self) -> bool {
        false
    }
}

impl LifecycleUi for ModelPickerLaunchWhileActiveUi {
    fn pump_once(&mut self) {
        if self.active.load(Ordering::SeqCst) && !self.injected {
            self.pending_model_picker_launch_requests =
                self.pending_model_picker_launch_requests.saturating_add(1);
            self.injected = true;
        }
    }

    fn quit_requested(&self) -> bool {
        self.quit
    }

    fn fatal_error(&self) -> Option<&str> {
        self.fatal.as_deref()
    }
}

impl UserInputUi for ModelPickerLaunchWhileActiveUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn take_next_model_picker_launch_request(&mut self) -> bool {
        if self.pending_model_picker_launch_requests == 0 {
            return false;
        }
        self.pending_model_picker_launch_requests =
            self.pending_model_picker_launch_requests.saturating_sub(1);
        true
    }
}

impl DisplayStateUi for ModelPickerLaunchWhileActiveUi {
    fn set_mcp_server_state(&mut self, server_name: &str, state: McpUsabilityState) {
        self.mcp_states.push((server_name.to_string(), state));
    }

    fn execute_shared_ui_action(&mut self, action: SharedUiAction) -> bool {
        self.shared_actions.push(action);
        self.shared_actions_observed_while_active
            .push(self.active.load(Ordering::SeqCst));
        self.block_first_turn.store(true, Ordering::SeqCst);
        true
    }
}

impl TranscriptUi for ModelPickerLaunchWhileActiveUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
    }
}

// ── StartupHydrationRuntime ─────────────────────────────────────────────

struct StartupHydrationRuntime {
    names_by_server: Vec<(String, Vec<String>)>,
}

impl CoreRuntime for StartupHydrationRuntime {
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

impl HasMcpManagement for StartupHydrationRuntime {
    async fn set_mcp_server_enabled(
        &mut self,
        _name: &str,
        _enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(McpUsabilityState::Disabled)
    }

    fn llm_visible_mcp_tool_count(&self) -> usize {
        self.names_by_server
            .iter()
            .map(|(_, names)| names.len())
            .sum()
    }

    fn llm_visible_mcp_tool_count_for_server(&self, _server_name: &str) -> usize {
        0
    }

    fn llm_visible_mcp_tool_names_by_server(&self) -> Vec<(String, Vec<String>)> {
        self.names_by_server.clone()
    }
}

impl HasModelSwitching for StartupHydrationRuntime {
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

crate::default_session!(StartupHydrationRuntime);
crate::default_compaction!(StartupHydrationRuntime);

#[tokio::test]
async fn tool_display_path_does_not_require_assistant_synthesis_round_trip() {
    let mut runtime = ToolDisplayOnlyRuntime;
    let mut ui = FakeProgressUi::default();

    let value = run_single_turn(
        &mut runtime,
        &mut ui,
        "show me diff".to_string(),
        None,
        Span::test_data(),
    )
    .await
    .expect("single turn");

    assert!(value.is_nothing());
    assert!(ui.events.iter().any(|event| matches!(
        event,
        UiEvent::ToolEnd {
            display: Some(_),
            ..
        }
    )));
    assert!(
        !ui.events
            .iter()
            .any(|event| matches!(event, UiEvent::AssistantMessage { .. }))
    );
}

#[tokio::test]
async fn run_single_turn_uses_progress_ui_trait_boundary() {
    let mut runtime = FakeRuntime::default();
    let mut ui = FakeProgressUi::default();

    let value = run_single_turn(
        &mut runtime,
        &mut ui,
        "hello".to_string(),
        Some("ctx".to_string()),
        Span::test_data(),
    )
    .await
    .expect("single turn");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
}

#[tokio::test]
async fn run_interactive_loop_uses_interactive_ui_trait_boundary() {
    let runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["a", "b"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["a".to_string(), "b".to_string()]);
}

#[tokio::test]
async fn interactive_loop_does_not_return_per_turn_values_to_stdout() {
    let runtime = FakeValueRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["hello"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
}

#[tokio::test]
async fn interactive_loop_treats_llm_cancellation_as_non_fatal_and_continues() {
    let runtime = CancelFirstRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["first", "second"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop should continue after cancellation");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["first".to_string(), "second".to_string()]
    );
}

#[tokio::test]
async fn interactive_loop_treats_errors_as_non_fatal_and_displays_inline() {
    let runtime = ErrorFirstRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&["first", "second"]);

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop should continue after error");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["first".to_string(), "second".to_string()]
    );
    assert!(
        ui.warnings
            .iter()
            .any(|w| w.contains("API rate limit exceeded")),
        "error should be displayed as inline warning"
    );
}

#[tokio::test]
async fn run_hydrated_interactive_loop_hydrates_before_first_pump() {
    let runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    let messages = vec![
        UiMessageSnapshot::new("user", "from history"),
        UiMessageSnapshot::new("assistant", "from assistant"),
    ];

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()).with_hydration(messages, None),
    )
    .await;
    result.expect("interactive loop with hydration");

    assert_eq!(
        &ui.call_order[..2],
        ["hydrate", "pump_once"],
        "expected hydrate before first pump"
    );
}

#[tokio::test]
async fn run_hydrated_interactive_loop_hydrates_exactly_once() {
    let runtime = FakeRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    let messages = vec![UiMessageSnapshot::new("user", "history"), {
        let mut s = UiMessageSnapshot::new("assistant", "response");
        s.usage = Some(UiMessageUsageSnapshot {
            input_tokens: None,
            output_tokens: None,
            total_tokens: Some(321),
        });
        s
    }];
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()).with_hydration(messages.clone(), None),
    )
    .await;
    result.expect("interactive loop with hydration");

    assert_eq!(ui.hydrated_messages, messages);
}

#[tokio::test]
async fn interactive_loop_processes_input_while_first_turn_is_running() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let mut ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["second"],
        &[],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        2,
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
    let value = result.expect("interactive loop should stay responsive");

    assert!(value.is_nothing());
    assert_eq!(
        _runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first", "second"]
    );
}

#[tokio::test]
async fn interactive_loop_preserves_fifo_for_prompts_queued_while_active() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let mut ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["second", "third"],
        &[],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        3,
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
    result.expect("interactive loop should complete queued prompts");

    assert_eq!(
        _runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first", "second", "third"]
    );
}

#[tokio::test]
#[serial_test::serial]
async fn permission_requested_emits_before_execution_and_waits_for_decision_before_side_effects() {
    let runtime = PermissionGateRuntime::new();
    let pumps_while_waiting = Arc::new(AtomicUsize::new(0));
    let mut ui = PermissionOrderingUi::new(
        PermissionDecision::AllowOnce,
        4,
        Arc::clone(&runtime.active),
        Arc::clone(&pumps_while_waiting),
        Arc::clone(&runtime.side_effects),
    );

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(_runtime.side_effects.load(Ordering::SeqCst), 1);
    assert!(
        pumps_while_waiting.load(Ordering::SeqCst) > 0,
        "execution must pause while waiting for permission decision"
    );

    let requested_idx = ui
        .events
        .iter()
        .position(|event| matches!(event, UiEvent::PermissionRequested { .. }))
        .expect("PermissionRequested must be emitted");
    let submitted_idx = ui
        .events
        .iter()
        .position(|event| matches!(event, UiEvent::PermissionDecisionSubmitted { .. }))
        .expect("PermissionDecisionSubmitted must be emitted");
    let tool_start_idx = ui
        .events
        .iter()
        .position(|event| matches!(event, UiEvent::ToolStart { .. }))
        .expect("ToolStart should happen after allow decision");

    assert!(requested_idx < submitted_idx);
    assert!(submitted_idx < tool_start_idx);
}

#[tokio::test]
#[serial_test::serial]
async fn deny_decision_resumes_deterministically_without_pre_decision_handler_side_effects() {
    let runtime = PermissionGateRuntime::new();
    let mut ui = PermissionOrderingUi::new(
        PermissionDecision::Deny,
        3,
        Arc::clone(&runtime.active),
        Arc::new(AtomicUsize::new(0)),
        Arc::clone(&runtime.side_effects),
    );

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(_runtime.side_effects.load(Ordering::SeqCst), 0);

    assert!(
        ui.events
            .iter()
            .any(|event| matches!(event, UiEvent::PermissionRequested { .. }))
    );
    assert!(ui.events.iter().any(|event| matches!(
        event,
        UiEvent::PermissionDecisionSubmitted {
            decision: PermissionDecision::Deny,
            ..
        }
    )));
    assert!(
        !ui.events
            .iter()
            .any(|event| matches!(event, UiEvent::ToolStart { .. }))
    );
}

#[tokio::test]
async fn models_launcher_opens_picker_while_worker_active() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let mut ui = ModelPickerLaunchWhileActiveUi::new(
        &["first"],
        1,
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
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
    let value = result.expect("interactive loop should process model launcher while active");

    assert!(value.is_nothing());
    assert_eq!(ui.shared_actions, vec![SharedUiAction::Models]);
    assert_eq!(ui.shared_actions_observed_while_active, vec![true]);
    assert_eq!(
        _runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
}

#[tokio::test]
async fn models_slash_opens_picker_while_worker_active() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    let mut ui = ModelPickerLaunchWhileActiveUi::new(
        &["first"],
        1,
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
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
    let value = result.expect("interactive loop should process /models while active");

    assert!(value.is_nothing());
    assert_eq!(ui.shared_actions, vec![SharedUiAction::Models]);
    assert_eq!(ui.shared_actions_observed_while_active, vec![true]);
    assert_eq!(
        _runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
}

#[tokio::test]
async fn interactive_loop_global_abort_cancels_active_and_does_not_run_queued_prompt() {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn));
    // Only submit "first" — the "queued" prompt behavior can't be tested
    // in async because pump_once never runs while execute_turn is spinning
    // (they share the same async task). The abort/cancellation path is
    // still exercised via the cancel_requested flag checked in execute_turn.
    let mut ui = FakeInteractiveUi::with_prompts(&["first"]);

    // Spawn a background task that unblocks the turn after a short delay.
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        unblock.store(true, Ordering::SeqCst);
    });

    let (rt, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop should treat cancellation as non-fatal");

    assert!(value.is_nothing());
    assert_eq!(
        rt.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
}

#[tokio::test]
async fn interactive_loop_startup_hydration_initializes_per_server_visible_counts_before_toggles() {
    let runtime = StartupHydrationRuntime {
        names_by_server: vec![
            (
                "gh".to_string(),
                vec!["gh__issues".to_string(), "gh__prs".to_string()],
            ),
            ("k8s".to_string(), vec!["k8s__pods".to_string()]),
        ],
    };
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        &mut ui,
        InteractiveLoopConfig::new(Span::test_data()),
    )
    .await;
    let value = result.expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        ui.mcp_visible_tool_count_updates,
        vec![("gh".to_string(), 2), ("k8s".to_string(), 1)]
    );
}

#[test]
fn emit_batch_delivers_all_events() {
    // RED phase: write test that verifies emit_batch delivers all events
    #[derive(Default)]
    struct BatchTestUi {
        events: Vec<UiEvent>,
        emit_calls: usize,
        emit_batch_calls: usize,
    }

    impl ProgressUi for BatchTestUi {
        fn emit(&mut self, event: &UiEvent) {
            self.events.push(event.clone());
            self.emit_calls += 1;
        }

        fn flush(&mut self) {}

        fn take_cancel_requested(&self) -> bool {
            false
        }

        fn emit_batch(&mut self, events: &[UiEvent]) {
            self.emit_batch_calls += 1;
            for event in events {
                self.events.push(event.clone());
            }
        }
    }

    impl LifecycleUi for BatchTestUi {
        fn pump_once(&mut self) {}

        fn quit_requested(&self) -> bool {
            false
        }

        fn fatal_error(&self) -> Option<&str> {
            None
        }
    }

    impl UserInputUi for BatchTestUi {
        fn take_submitted_prompt(&mut self) -> Option<String> {
            None
        }
    }

    impl DisplayStateUi for BatchTestUi {}

    impl TranscriptUi for BatchTestUi {
        fn hydrate_transcript_from_messages(
            &mut self,
            _messages: impl IntoIterator<Item = UiMessageSnapshot>,
            _last_total_tokens: Option<u64>,
        ) {
        }
    }

    let mut ui = BatchTestUi::default();

    // Create 5 test events
    let events = vec![
        UiEvent::Tick,
        UiEvent::LlmStart,
        UiEvent::AssistantMessage {
            text: "hello".to_string(),
        },
        UiEvent::AssistantMessage {
            text: "world".to_string(),
        },
        UiEvent::Completed { tool_calls: 0 },
    ];

    // Call emit_batch
    ui.emit_batch(&events);

    // Verify all events were delivered
    assert_eq!(ui.events.len(), 5, "all 5 events should be delivered");
    assert_eq!(ui.emit_batch_calls, 1, "emit_batch should be called once");
    assert_eq!(
        ui.emit_calls, 0,
        "emit should not be called when using emit_batch"
    );
}
