use super::test_shared::*;
use crate::mailbox::IncomingMessage;

// ── ContextWindowRuntime ────────────────────────────────────────────────

struct ContextWindowRuntime {
    switched_models: Vec<String>,
    max_context_tokens: Option<u64>,
}

impl Default for ContextWindowRuntime {
    fn default() -> Self {
        Self {
            switched_models: Vec::new(),
            max_context_tokens: Some(128_000),
        }
    }
}

impl CoreRuntime for ContextWindowRuntime {
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

impl HasMcpManagement for ContextWindowRuntime {
    fn set_mcp_server_enabled(
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

impl HasModelSwitching for ContextWindowRuntime {
    fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String> {
        self.switched_models.push(model_spec.to_string());
        Ok((model_spec.to_string(), self.max_context_tokens))
    }

    fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported in this runtime".to_string())
    }

    fn active_model_identity(&self) -> String {
        "openai/gpt-4o-mini".to_string()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        self.max_context_tokens
    }
}

crate::default_session!(ContextWindowRuntime);
crate::default_compaction!(ContextWindowRuntime);

// ── ContextWindowUi ─────────────────────────────────────────────────────

struct ContextWindowUi {
    submitted: std::collections::VecDeque<String>,
    model_switch_requests: std::collections::VecDeque<String>,
    quit: bool,
    pump_count: usize,
    warnings: Vec<String>,
    active_model_identity: Option<String>,
    context_window_max_tokens: Option<Option<u64>>,
}

impl ContextWindowUi {
    fn new(model_switch_requests: &[&str]) -> Self {
        Self {
            submitted: std::collections::VecDeque::new(),
            model_switch_requests: model_switch_requests
                .iter()
                .map(|s| s.to_string())
                .collect(),
            quit: false,
            pump_count: 0,
            warnings: Vec::new(),
            active_model_identity: None,
            context_window_max_tokens: None,
        }
    }
}

impl ProgressUi for ContextWindowUi {
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

impl LifecycleUi for ContextWindowUi {
    fn pump_once(&mut self) {
        self.pump_count = self.pump_count.saturating_add(1);
        if self.model_switch_requests.is_empty() && self.pump_count > 1 {
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

impl UserInputUi for ContextWindowUi {
    fn take_submitted_prompt(&mut self) -> Option<String> {
        self.submitted.pop_front()
    }

    fn take_next_model_switch_request(&mut self) -> Option<String> {
        self.model_switch_requests.pop_front()
    }
}

impl DisplayStateUi for ContextWindowUi {
    fn set_active_model_identity(&mut self, active_model_identity: &str) {
        self.active_model_identity = Some(active_model_identity.to_string());
    }

    fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.context_window_max_tokens = Some(max_tokens);
    }

    fn set_mcp_server_state(&mut self, _server_name: &str, _state: McpUsabilityState) {}

    fn execute_shared_ui_action(&mut self, _action: SharedUiAction) -> bool {
        true
    }
}

impl TranscriptUi for ContextWindowUi {
    fn hydrate_transcript_from_messages(
        &mut self,
        _messages: impl IntoIterator<Item = UiMessageSnapshot>,
        _last_total_tokens: Option<u64>,
    ) {
    }
}

// ── TokenSeedingRuntime ─────────────────────────────────────────────────

#[derive(Default)]
struct TokenSeedingRuntime {
    seeded_tokens: Option<Option<u64>>,
}

impl CoreRuntime for TokenSeedingRuntime {
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

impl HasModelSwitching for TokenSeedingRuntime {
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

impl HasSessionManagement for TokenSeedingRuntime {
    fn clear_session(&mut self) {}

    fn new_session(&mut self) {}

    fn seed_last_total_tokens(&mut self, tokens: Option<u64>) {
        self.seeded_tokens = Some(tokens);
    }
}

crate::default_mcp!(TokenSeedingRuntime);
crate::default_compaction!(TokenSeedingRuntime);

#[test]
fn orchestrator_formats_task_kind() {
    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "parent".to_string(),
        message: "build the auth module".to_string(),
        kind: "task".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["[TASK from: parent] build the auth module".to_string()]
    );
}

#[test]
fn orchestrator_formats_completion_kind() {
    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "worker-1".to_string(),
        message: "auth module is done".to_string(),
        kind: "completion".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["[COMPLETED from: worker-1] auth module is done".to_string()]
    );
}

#[test]
fn orchestrator_formats_question_kind() {
    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "worker-2".to_string(),
        message: "which database should I use?".to_string(),
        kind: "question".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts.len(), 1);
    assert!(
        runtime.prompts[0].starts_with("[QUESTION from: worker-2"),
        "question kind should start with [QUESTION from: ...]: {}",
        runtime.prompts[0]
    );
    assert!(
        runtime.prompts[0].contains("BLOCKED"),
        "question kind should contain BLOCKED: {}",
        runtime.prompts[0]
    );
}

#[test]
fn orchestrator_formats_default_kind() {
    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "peer".to_string(),
        message: "general info".to_string(),
        kind: "message".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["[from: peer] general info".to_string()]
    );
}

#[test]
fn orchestrator_formats_unknown_kind_as_default() {
    let mut runtime = MailboxTestRuntime::default();
    let mut ui = MailboxTestUi::with_prompts(&[]);

    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(IncomingMessage {
        from: "peer".to_string(),
        message: "something custom".to_string(),
        kind: "custom_kind".to_string(),
    })
    .unwrap();

    let value = run_interactive_loop(&mut runtime, &mut ui, Some(rx), Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["[from: peer] something custom".to_string()]
    );
}

#[test]
fn context_window_max_tokens_set_on_ui_at_startup() {
    let mut runtime = ContextWindowRuntime::default(); // max_context_tokens = Some(128_000)
    let mut ui = ContextWindowUi::new(&[]);

    run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert_eq!(
        ui.context_window_max_tokens,
        Some(Some(128_000)),
        "expected context_window_max_tokens to be set to Some(128_000) at startup"
    );
}

#[test]
fn model_switch_updates_context_window_max_tokens_in_ui() {
    let mut runtime = ContextWindowRuntime::default(); // max_context_tokens = Some(128_000)
    let mut ui = ContextWindowUi::new(&["openai/gpt-4o-mini"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    // context_window_max_tokens must have been set on the UI
    assert_eq!(
        ui.context_window_max_tokens,
        Some(Some(128_000)),
        "expected context_window_max_tokens to be updated with 128_000 after model switch"
    );
}

#[test]
fn model_switch_updates_context_window_max_tokens_none_when_unset() {
    let mut runtime = ContextWindowRuntime {
        max_context_tokens: None,
        ..Default::default()
    };
    let mut ui = ContextWindowUi::new(&["openai/gpt-4o-mini"]);

    let value = run_interactive_loop(&mut runtime, &mut ui, None, Span::test_data(), None)
        .expect("interactive loop");

    assert!(value.is_nothing());
    assert_eq!(
        ui.context_window_max_tokens,
        Some(None),
        "expected context_window_max_tokens to be set to None when model has no limit"
    );
}

#[test]
fn session_resume_seeds_last_total_tokens() {
    let mut runtime = TokenSeedingRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    run_hydrated_interactive_loop(
        &mut runtime,
        &mut ui,
        vec![],
        Some(90_000),
        None,
        Span::test_data(),
        None,
    )
    .expect("hydrated interactive loop");

    assert_eq!(
        runtime.seeded_tokens,
        Some(Some(90_000)),
        "seed_last_total_tokens must be called with the loaded token count on session resume"
    );
}

#[test]
fn session_resume_seeds_last_total_tokens_none_when_no_prior_session() {
    let mut runtime = TokenSeedingRuntime::default();
    let mut ui = FakeInteractiveUi::with_prompts(&[]);

    run_hydrated_interactive_loop(
        &mut runtime,
        &mut ui,
        vec![],
        None,
        None,
        Span::test_data(),
        None,
    )
    .expect("hydrated interactive loop");

    assert_eq!(
        runtime.seeded_tokens,
        Some(None),
        "seed_last_total_tokens must be called with None when no prior token count exists"
    );
}
