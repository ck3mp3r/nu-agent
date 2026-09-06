use super::test_shared::*;
use crate::protocol::contracts::ProgressUi;
use crate::protocol::event::{PermissionDecisionSubmission, UiEvent};

type TResult = core::result::Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn recognized_slash_commands_never_sent_to_llm() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[
        "/help", "/status", "/mcp", "/models", "/agent", "/compact", "/skills",
    ])
    .with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.run_compaction_calls, 1);
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn new_slash_command_clears_transcript_and_pushes_startup_logo() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/new"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(ui.clear_transcript_count.load(Ordering::SeqCst), 1);
    assert_eq!(ui.push_startup_logo_count.load(Ordering::SeqCst), 1);
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn models_slash_command_not_sent_to_llm() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/models"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn models_slash_command_routes_to_shared_models_action() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/models"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        ui.shared_actions.lock().unwrap().clone(),
        vec![SharedUiAction::Models]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_routes_compact_slash_to_compaction_executor() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/compact", "hello"])
        .with_min_bus_events(2)
        .with_expected_compaction_events(1)
        .with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.run_compaction_calls, 1);
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    Ok(())
}

#[tokio::test]
async fn typed_compact_submit_triggers_compaction_path() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/compact"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.run_compaction_calls, 1);
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn interactive_loop_unknown_slash_emits_warning_and_continues() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui =
        FakeInteractiveUi::with_prompts(&["/compact now", "real prompt"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert!(
        ui.warnings
            .lock()
            .unwrap()
            .iter()
            .any(|entry| entry == "Unknown slash command: /compact now")
    );
    assert_eq!(runtime.prompts, vec!["real prompt".to_string()]);
    Ok(())
}

#[tokio::test]
async fn recognized_slash_commands_not_persisted_as_session_turn_messages() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/help", "/status", "/mcp", "/compact"])
        .with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.run_compaction_calls, 1,
        "only /compact should route to session compaction"
    );
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn manual_compaction_failure_is_not_surfaced_as_warning() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        run_compaction_fail: true,
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/compact"])
        .with_expected_compaction_events(0)
        .with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.run_compaction_calls, 1,
        "expected run_compaction to be invoked"
    );
    assert_eq!(
        runtime.run_compaction_sources,
        vec!["slash".to_string()],
        "expected /compact to dispatch RunCompaction with source 'slash'"
    );
    assert!(
        ui.warnings
            .lock()
            .unwrap()
            .iter()
            .all(|w| { w != "auto compaction failed" }),
        "a failed compaction must not surface as a warning (it is fire-and-forget via the bus)"
    );
    Ok(())
}

#[tokio::test]
async fn slash_commands_reuse_command_palette_action_handlers() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[
        "/help", "/status", "/mcp", "/models", "/agent", "/skills",
    ])
    .with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        ui.shared_actions.lock().unwrap().clone(),
        vec![
            SharedUiAction::Help,
            SharedUiAction::Status,
            SharedUiAction::Mcps,
            SharedUiAction::Models,
            SharedUiAction::Agents,
            SharedUiAction::Skills,
        ]
    );
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn command_palette_models_action_opens_inline_model_picker() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/models"])
        .with_min_bus_events(2)
        .with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        ui.shared_actions.lock().unwrap().clone(),
        vec![SharedUiAction::Models]
    );
    Ok(())
}

#[tokio::test]
async fn manual_compaction_slash_works_with_turn_processing() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["hello", "/compact"])
        .with_min_bus_events(2)
        .with_expected_compaction_events(1)
        .with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.run_compaction_calls, 1);
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    Ok(())
}

// ── ContextWindowRuntime ────────────────────────────────────────────────

struct ContextWindowRuntime {
    switched_models: Vec<String>,
    max_context_tokens: Option<u64>,
    bus: crate::bus::Bus,
}

impl Default for ContextWindowRuntime {
    fn default() -> Self {
        Self {
            switched_models: Vec::new(),
            max_context_tokens: Some(128_000),
            bus: crate::bus::Bus::default(),
        }
    }
}

impl CoreRuntime for ContextWindowRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        let _ = self
            .bus
            .turn()
            .send(crate::bus::TurnEvent::Completed { tool_calls: 0 })
            .await;
        Ok(Value::nothing(span))
    }
}

impl McpManagement for ContextWindowRuntime {
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

impl ModelSwitching for ContextWindowRuntime {
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

// ── ContextWindowUi ─────────────────────────────────────────────────────

struct ContextWindowUi {
    event_tx: mpsc::Sender<OrchestratorEvent>,
    model_switch_requests: Arc<Mutex<std::collections::VecDeque<String>>>,
    quit: Arc<AtomicBool>,
    warnings: Arc<Mutex<Vec<String>>>,
    context_window_max_tokens: Arc<Mutex<Option<Option<u64>>>>,
    ui_state_event_count: Arc<AtomicUsize>,
    min_ui_state_events: usize,
    _bus_task: Option<tokio::task::JoinHandle<()>>,
}

impl ContextWindowUi {
    fn new(model_switch_requests: &[&str]) -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
        Self {
            event_tx,
            model_switch_requests: Arc::new(Mutex::new(
                model_switch_requests
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            )),
            quit: Arc::new(AtomicBool::new(false)),
            warnings: Arc::new(Mutex::new(Vec::new())),
            context_window_max_tokens: Arc::new(Mutex::new(None)),
            ui_state_event_count: Arc::new(AtomicUsize::new(0)),
            min_ui_state_events: 0,
            _bus_task: None,
        }
    }

    fn with_min_ui_state_events(mut self, min_ui_state_events: usize) -> Self {
        self.min_ui_state_events = min_ui_state_events;
        self
    }

    fn make_event_spawner(&self) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let model_switch_requests = Arc::clone(&self.model_switch_requests);
        let quit = Arc::clone(&self.quit);
        move |event_tx| {
            let event_tx = event_tx.clone();
            tokio::spawn(async move {
                loop {
                    let spec = model_switch_requests.lock().expect("ms lock").pop_front();
                    if let Some(spec) = spec {
                        let _ = event_tx
                            .send(OrchestratorEvent::UiRequest(UiRequest::SwitchModel {
                                spec,
                            }))
                            .await;
                    }
                    if quit.load(Ordering::SeqCst) {
                        let _ = event_tx.send(OrchestratorEvent::Quit).await;
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            });
        }
    }

    fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let model_switch_requests = Arc::clone(&self.model_switch_requests);
        let quit = Arc::clone(&self.quit);
        let warnings = Arc::clone(&self.warnings);
        let context_window_max_tokens = Arc::clone(&self.context_window_max_tokens);
        let ui_state_event_count = Arc::clone(&self.ui_state_event_count);
        let min_ui_state_events = self.min_ui_state_events;

        let mut turn_rx = bus.turn().subscribe();
        let mut ui_state_rx = bus.ui_state().subscribe();
        let mut warning_rx = bus.warning().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(_event) = turn_rx.recv() => {}
                    Ok(event) = ui_state_rx.recv() => {
                        let count = ui_state_event_count.fetch_add(1, Ordering::SeqCst) + 1;
                        if let UiStateEvent::SetContextWindowMaxTokens(max_tokens) = event {
                            *context_window_max_tokens.lock().expect("context window lock") = Some(max_tokens);
                        }
                        let empty = model_switch_requests.lock().expect("model switch lock").is_empty();
                        if empty && count > min_ui_state_events {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    Ok(event) = warning_rx.recv() => {
                        match event {
                            crate::bus::WarningEvent::Message { message }
                            | crate::bus::WarningEvent::TurnError { message } => {
                                warnings.lock().expect("warnings lock").push(message);
                            }
                        }
                    }
                    else => break,
                }
            }
        }));
        self
    }
}

impl UserInputUi for ContextWindowUi {
    fn event_sender(&self) -> &mpsc::Sender<OrchestratorEvent> {
        &self.event_tx
    }
}

// ── TokenSeedingRuntime ─────────────────────────────────────────────────

#[derive(Default)]
struct TokenSeedingRuntime {
    seeded_tokens: Option<Option<u64>>,
    bus: crate::bus::Bus,
}

impl CoreRuntime for TokenSeedingRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        let _ = self
            .bus
            .turn()
            .send(crate::bus::TurnEvent::Completed { tool_calls: 0 })
            .await;
        Ok(Value::nothing(span))
    }
}

impl ModelSwitching for TokenSeedingRuntime {
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

impl SessionState for TokenSeedingRuntime {
    fn clear_session(&mut self) {}

    fn new_session(&mut self) {}

    fn seed_last_total_tokens(&mut self, tokens: Option<u64>) {
        self.seeded_tokens = Some(tokens);
    }
}
impl SessionPersistence for TokenSeedingRuntime {}

crate::default_mcp!(TokenSeedingRuntime);

#[tokio::test]
async fn context_window_max_tokens_set_on_ui_at_startup() -> TResult {
    let bus = create_bus();
    let runtime = ContextWindowRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = ContextWindowUi::new(&[])
        .with_min_ui_state_events(1)
        .with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("interactive loop: {e}"))?;

    assert_eq!(
        *ui.context_window_max_tokens.lock().unwrap(),
        Some(Some(128_000)),
        "expected context_window_max_tokens to be set to Some(128_000) at startup"
    );
    Ok(())
}

#[tokio::test]
async fn model_switch_updates_context_window_max_tokens_in_ui() -> TResult {
    let bus = create_bus();
    let runtime = ContextWindowRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = ContextWindowUi::new(&["openai/gpt-4o-mini"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    assert_eq!(
        *ui.context_window_max_tokens.lock().unwrap(),
        Some(Some(128_000)),
        "expected context_window_max_tokens to be updated with 128_000 after model switch"
    );
    Ok(())
}

#[tokio::test]
async fn model_switch_updates_context_window_max_tokens_none_when_unset() -> TResult {
    let bus = create_bus();
    let runtime = ContextWindowRuntime {
        max_context_tokens: None,
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = ContextWindowUi::new(&["openai/gpt-4o-mini"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        *ui.context_window_max_tokens.lock().unwrap(),
        Some(None),
        "expected context_window_max_tokens to be set to None when model has no limit"
    );
    Ok(())
}

#[tokio::test]
async fn session_resume_seeds_last_total_tokens() -> TResult {
    let bus = create_bus();
    let runtime = TokenSeedingRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_hydration(vec![], Some(90_000))
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("hydrated interactive loop: {e}"))?;

    assert_eq!(
        runtime.seeded_tokens,
        Some(Some(90_000)),
        "seed_last_total_tokens must be called with the loaded token count on session resume"
    );
    Ok(())
}

#[tokio::test]
async fn session_resume_seeds_last_total_tokens_none_when_no_prior_session() -> TResult {
    let bus = create_bus();
    let runtime = TokenSeedingRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_hydration(vec![], None)
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("hydrated interactive loop: {e}"))?;

    assert_eq!(
        runtime.seeded_tokens,
        Some(None),
        "seed_last_total_tokens must be called with None when no prior token count exists"
    );
    Ok(())
}

// ── ToolDisplayOnlyRuntime ──────────────────────────────────────────────

#[derive(Default)]
struct ToolDisplayOnlyRuntime {
    bus: crate::bus::Bus,
}

impl CoreRuntime for ToolDisplayOnlyRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        let _ = self
            .bus
            .tool()
            .send(crate::bus::ToolEvent::Started {
                name: "edit".to_string(),
                source: "closure".to_string(),
                arguments: "{}".to_string(),
            })
            .await;
        let _ = self
            .bus
            .tool()
            .send(crate::bus::ToolEvent::Completed {
                name: "edit".to_string(),
                source: "closure".to_string(),
                arguments: "{}".to_string(),
                success: true,
                result: r#"{"path":"file.txt","diff":"--- a/file.txt\n+++ b/file.txt\n"}"#
                    .to_string(),
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
            })
            .await;
        let _ = self
            .bus
            .turn()
            .send(crate::bus::TurnEvent::Completed { tool_calls: 1 })
            .await;
        Ok(Value::nothing(span))
    }
}

impl ModelSwitching for ToolDisplayOnlyRuntime {
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
    bus: crate::bus::Bus,
}

impl CoreRuntime for CancelFirstRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        let _ = self
            .bus
            .turn()
            .send(crate::bus::TurnEvent::Completed { tool_calls: 0 })
            .await;
        if self.prompts.len() == 1 {
            return Err(LabeledError::new("LLM call cancelled"));
        }
        Ok(Value::nothing(Span::test_data()))
    }
}

impl ModelSwitching for CancelFirstRuntime {
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

// ── ErrorFirstRuntime ───────────────────────────────────────────────────

#[derive(Default)]
struct ErrorFirstRuntime {
    prompts: Vec<String>,
    bus: crate::bus::Bus,
}

impl CoreRuntime for ErrorFirstRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        let _ = self
            .bus
            .turn()
            .send(crate::bus::TurnEvent::Completed { tool_calls: 0 })
            .await;
        if self.prompts.len() == 1 {
            return Err(LabeledError::new("API rate limit exceeded"));
        }
        Ok(Value::nothing(Span::test_data()))
    }
}

impl ModelSwitching for ErrorFirstRuntime {
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

// ── PermissionGateRuntime ───────────────────────────────────────────────

struct PermissionGateRuntime {
    side_effects: Arc<AtomicUsize>,
    requested: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    request_id: String,
    rule_identity: String,
    pending: crate::conversation::runtime::PendingPermissions,
    bus: crate::bus::Bus,
}

impl PermissionGateRuntime {
    fn new() -> Self {
        Self {
            side_effects: Arc::new(AtomicUsize::new(0)),
            requested: Arc::new(AtomicBool::new(false)),
            finished: Arc::new(AtomicBool::new(false)),
            active: Arc::new(AtomicBool::new(false)),
            request_id: "ask-0000000000000abc".to_string(),
            rule_identity: "nested:nu.command:*".to_string(),
            pending: Arc::new(Mutex::new(std::collections::HashMap::new())),
            bus: crate::bus::Bus::default(),
        }
    }

    fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        self.bus = bus;
        self
    }
}

impl CoreRuntime for PermissionGateRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        self.active.store(true, Ordering::SeqCst);

        // Publish a permission request on the bus and register a oneshot in the
        // shared pending map, mirroring production `InteractivePermissionResolver`.
        let request_id = self.request_id.clone();
        let context = crate::protocol::event::PermissionRequestContext {
            tool: "nu".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            matched_rule_identity: self.rule_identity.clone(),
            scope: "nested".to_string(),
            target_field: Some("command".to_string()),
            pattern: "*".to_string(),
            summary: "→ {\"command\":\"echo hi\"}".to_string(),
            pre_authorize_display: None,
        };
        let (tx, rx) = crate::bus::OneshotTx::channel("permission");
        self.pending
            .lock()
            .expect("pending lock")
            .insert(request_id.clone(), tx);
        let _ = self
            .bus
            .permission()
            .send(crate::bus::PermissionEvent::Requested {
                request_id: request_id.clone(),
                context: Box::new(context),
            })
            .await;
        self.requested.store(true, Ordering::SeqCst);

        // Await the UI's decision via the oneshot. Deny on channel drop.
        let decision = rx.await.unwrap_or(PermissionDecision::Deny);

        if decision != PermissionDecision::Deny {
            self.side_effects.fetch_add(1, Ordering::SeqCst);
            // Publish the tool-start to the bus tool channel directly, matching
            // production where the hook publishes ToolEvent::Started.
            let _ = self
                .bus
                .tool()
                .send(crate::bus::ToolEvent::Started {
                    name: "nu".to_string(),
                    source: "closure".to_string(),
                    arguments: r#"{"command":"echo hi"}"#.to_string(),
                })
                .await;
        }

        // Publish TurnCompleted directly to the bus (worker bridge no longer
        // converts UiEvent::Completed), matching production.
        let _ = self
            .bus
            .turn()
            .send(crate::bus::TurnEvent::Completed { tool_calls: 0 })
            .await;
        self.finished.store(true, Ordering::SeqCst);
        self.active.store(false, Ordering::SeqCst);
        Ok(Value::nothing(span))
    }
}

impl McpManagement for PermissionGateRuntime {
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

impl ModelSwitching for PermissionGateRuntime {
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
    event_tx: mpsc::Sender<OrchestratorEvent>,
    submitted: std::collections::VecDeque<String>,
    pending_decisions: Arc<Mutex<std::collections::VecDeque<PermissionDecisionSubmission>>>,
    events: Arc<Mutex<Vec<UiEvent>>>,
    decision: PermissionDecision,
    decision_delay_pumps: usize,
    elapsed_pumps: Arc<AtomicUsize>,
    request_seen: Arc<AtomicBool>,
    quit: Arc<AtomicBool>,
    pumps_while_waiting: Arc<AtomicUsize>,
    _background_tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl PermissionOrderingUi {
    fn new(
        decision: PermissionDecision,
        decision_delay_pumps: usize,
        pumps_while_waiting: Arc<AtomicUsize>,
    ) -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
        let request_seen = Arc::new(AtomicBool::new(false));
        let elapsed_pumps = Arc::new(AtomicUsize::new(0));
        let rs = Arc::clone(&request_seen);
        let ep = Arc::clone(&elapsed_pumps);
        let background_delay = tokio::spawn(async move {
            loop {
                if rs.load(Ordering::SeqCst) {
                    ep.fetch_add(1, Ordering::SeqCst);
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        });
        Self {
            event_tx,
            submitted: ["run".to_string()].into_iter().collect(),
            pending_decisions: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            events: Arc::new(Mutex::new(Vec::new())),
            decision,
            decision_delay_pumps,
            elapsed_pumps,
            request_seen,
            quit: Arc::new(AtomicBool::new(false)),
            pumps_while_waiting,
            _background_tasks: vec![background_delay],
        }
    }

    fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let pending_decisions = Arc::clone(&self.pending_decisions);
        let events = Arc::clone(&self.events);
        let request_seen = Arc::clone(&self.request_seen);
        let quit = Arc::clone(&self.quit);
        let decision = self.decision;

        let mut permission_rx = bus.permission().subscribe();
        let mut turn_rx = bus.turn().subscribe();
        let mut tool_rx = bus.tool().subscribe();

        self._background_tasks.push(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = permission_rx.recv() => {
                        if let crate::bus::PermissionEvent::Requested { ref request_id, ref context } = event {
                            request_seen.store(true, Ordering::SeqCst);
                            pending_decisions.lock().expect("pending decisions lock").push_back(PermissionDecisionSubmission {
                                request_id: request_id.clone(),
                                decision,
                                matched_rule_identity: context.matched_rule_identity.clone(),
                            });
                            events.lock().expect("events lock").push(UiEvent::PermissionRequested {
                                request_id: request_id.clone(),
                                context: context.as_ref().clone(),
                            });
                        }
                        if let crate::bus::PermissionEvent::DecisionTimedOut { ref request_id } = event {
                            events.lock().expect("events lock").push(UiEvent::PermissionDecisionTimedOut { request_id: request_id.clone() });
                        }
                        if let crate::bus::PermissionEvent::DecisionIgnored { ref request_id, ref reason } = event {
                            events.lock().expect("events lock").push(UiEvent::PermissionDecisionIgnored { request_id: request_id.clone(), reason: reason.clone() });
                        }
                    }
                    Ok(event) = turn_rx.recv() => {
                        if let crate::bus::TurnEvent::Completed { .. } = event {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    Ok(event) = tool_rx.recv() => {
                        if let crate::bus::ToolEvent::Started { name, source, arguments } = event {
                            events.lock().expect("events lock").push(UiEvent::ToolStarted {
                                name,
                                source,
                                arguments,
                            });
                        }
                    }
                    else => break,
                }
            }
        }));
        self
    }

    fn make_event_spawner(&self) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let submitted = self.submitted.clone();
        let pending_decisions = Arc::clone(&self.pending_decisions);
        let quit = Arc::clone(&self.quit);
        let decision_delay_pumps = self.decision_delay_pumps;
        let elapsed_pumps = Arc::clone(&self.elapsed_pumps);
        let pumps_while_waiting = Arc::clone(&self.pumps_while_waiting);
        move |event_tx| {
            let event_tx = event_tx.clone();
            let mut submitted = submitted.clone();
            tokio::spawn(async move {
                loop {
                    if let Some(prompt) = submitted.pop_front() {
                        let _ = event_tx
                            .send(OrchestratorEvent::PromptSubmitted { text: prompt })
                            .await;
                    }
                    if elapsed_pumps.load(Ordering::SeqCst) < decision_delay_pumps {
                        pumps_while_waiting.fetch_add(1, Ordering::SeqCst);
                    } else {
                        let decision = pending_decisions.lock().expect("pending lock").pop_front();
                        if let Some(decision) = decision {
                            let _ = event_tx
                                .send(OrchestratorEvent::PermissionDecision { decision })
                                .await;
                        }
                    }
                    if quit.load(Ordering::SeqCst) {
                        let _ = event_tx.send(OrchestratorEvent::Quit).await;
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            });
        }
    }
}

impl UserInputUi for PermissionOrderingUi {
    fn event_sender(&self) -> &mpsc::Sender<OrchestratorEvent> {
        &self.event_tx
    }
}

// ── ModelPickerLaunchWhileActiveUi ──────────────────────────────────────

struct ModelPickerLaunchWhileActiveUi {
    event_tx: mpsc::Sender<OrchestratorEvent>,
    submitted: std::collections::VecDeque<String>,
    pending_model_picker_launch_requests: usize,
    quit: Arc<AtomicBool>,
    completed_count: Arc<AtomicUsize>,
    expected_completions: usize,
    shared_actions: Arc<Mutex<Vec<SharedUiAction>>>,
    _bus_task: Option<tokio::task::JoinHandle<()>>,
}

impl ModelPickerLaunchWhileActiveUi {
    fn new(initial_prompts: &[&str], expected_completions: usize) -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
        Self {
            event_tx,
            submitted: initial_prompts.iter().map(|s| s.to_string()).collect(),
            pending_model_picker_launch_requests: expected_completions,
            quit: Arc::new(AtomicBool::new(false)),
            completed_count: Arc::new(AtomicUsize::new(0)),
            expected_completions,
            shared_actions: Arc::new(Mutex::new(Vec::new())),
            _bus_task: None,
        }
    }

    fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let quit = Arc::clone(&self.quit);
        let completed_count = Arc::clone(&self.completed_count);
        let shared_actions = Arc::clone(&self.shared_actions);
        let expected_completions = self.expected_completions;

        let mut turn_rx = bus.turn().subscribe();
        let mut ui_state_rx = bus.ui_state().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = turn_rx.recv() => {
                        if let crate::bus::TurnEvent::Completed { .. } = event {
                            let count = completed_count.fetch_add(1, Ordering::SeqCst) + 1;
                            if count >= expected_completions {
                                quit.store(true, Ordering::SeqCst);
                            }
                        }
                    }
                    Ok(event) = ui_state_rx.recv() => {
                        if let UiStateEvent::ExecuteSharedUiAction(action) = event {
                            shared_actions.lock().expect("shared actions lock").push(action);
                        }
                    }
                    else => break,
                }
            }
        }));
        self
    }

    fn make_event_spawner(
        &self,
        bus: crate::bus::Bus,
    ) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let submitted = self.submitted.clone();
        let quit = Arc::clone(&self.quit);
        let pending_launches = self.pending_model_picker_launch_requests;
        let ui_state_tx = bus.ui_state().clone();
        move |event_tx| {
            let event_tx = event_tx.clone();
            let mut submitted = submitted.clone();
            tokio::spawn(async move {
                for _ in 0..pending_launches {
                    let _ = ui_state_tx
                        .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Models))
                        .await;
                }
                loop {
                    if let Some(prompt) = submitted.pop_front() {
                        let _ = event_tx
                            .send(OrchestratorEvent::PromptSubmitted { text: prompt })
                            .await;
                    }
                    if quit.load(Ordering::SeqCst) {
                        let _ = event_tx.send(OrchestratorEvent::Quit).await;
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            });
        }
    }
}

impl UserInputUi for ModelPickerLaunchWhileActiveUi {
    fn event_sender(&self) -> &mpsc::Sender<OrchestratorEvent> {
        &self.event_tx
    }
}

// ── StartupHydrationRuntime ─────────────────────────────────────────────

struct StartupHydrationRuntime {
    names_by_server: Vec<(String, Vec<String>)>,
    bus: crate::bus::Bus,
}

impl CoreRuntime for StartupHydrationRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        let _ = self
            .bus
            .turn()
            .send(crate::bus::TurnEvent::Completed { tool_calls: 0 })
            .await;
        Ok(Value::nothing(span))
    }
}

impl McpManagement for StartupHydrationRuntime {
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

impl ModelSwitching for StartupHydrationRuntime {
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

#[tokio::test]
async fn tool_display_path_does_not_require_assistant_synthesis_round_trip() -> TResult {
    let bus = create_bus();
    let mut runtime = ToolDisplayOnlyRuntime { bus: bus.clone() };
    let mut tool_rx = bus.tool().subscribe();

    let value = run_single_turn(
        &mut runtime,
        &bus,
        "show me diff".to_string(),
        None,
        Span::test_data(),
    )
    .await
    .map_err(|e| format!("single turn: {e}"))?;

    assert!(value.is_nothing());

    // Drain the bus tool channel (events published by the runtime) and convert
    // to UiEvent at the boundary.
    let mut events = Vec::new();
    loop {
        match tool_rx.try_recv() {
            Ok(event) => {
                if let Some(e) = Option::<UiEvent>::from(event) {
                    events.push(e);
                }
            }
            Err(crate::bus::TryRecvError::Empty) => break,
            Err(crate::bus::TryRecvError::Lagged(_)) => continue,
            Err(crate::bus::TryRecvError::Closed) => break,
        }
    }

    assert!(events.iter().any(|event| matches!(
        event,
        UiEvent::ToolCompleted {
            display: Some(_),
            ..
        }
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, UiEvent::AssistantMessage { .. }))
    );
    Ok(())
}

#[tokio::test]
async fn run_single_turn_uses_progress_ui_trait_boundary() -> TResult {
    let bus = create_bus();
    let mut runtime = FakeRuntime::default();

    let value = run_single_turn(
        &mut runtime,
        &bus,
        "hello".to_string(),
        Some("ctx".to_string()),
        Span::test_data(),
    )
    .await
    .map_err(|e| format!("single turn: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    Ok(())
}

#[tokio::test]
async fn run_interactive_loop_uses_interactive_ui_trait_boundary() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["a", "b"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["a".to_string(), "b".to_string()]);
    Ok(())
}

#[tokio::test]
async fn interactive_loop_does_not_return_per_turn_values_to_stdout() -> TResult {
    let bus = create_bus();
    let runtime = FakeValueRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["hello"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.prompts, vec!["hello".to_string()]);
    Ok(())
}

#[tokio::test]
async fn interactive_loop_treats_llm_cancellation_as_non_fatal_and_continues() -> TResult {
    let bus = create_bus();
    let runtime = CancelFirstRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["first", "second"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value =
        result.map_err(|e| format!("interactive loop should continue after cancellation: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["first".to_string(), "second".to_string()]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_treats_errors_as_non_fatal_and_displays_inline() -> TResult {
    let bus = create_bus();
    let runtime = ErrorFirstRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["first", "second"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop should continue after error: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.prompts,
        vec!["first".to_string(), "second".to_string()]
    );
    assert!(
        ui.warnings
            .lock()
            .unwrap()
            .iter()
            .any(|w| w.contains("API rate limit exceeded")),
        "error should be displayed as inline warning"
    );
    Ok(())
}

#[tokio::test]
async fn run_hydrated_interactive_loop_hydrates_before_first_pump() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());

    let messages = vec![
        UiMessageSnapshot::new("user", "from history"),
        UiMessageSnapshot::new("assistant", "from assistant"),
    ];

    let spawner = ui.make_event_spawner();
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_hydration(messages, None)
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("interactive loop with hydration: {e}"))?;

    assert_eq!(
        &ui.call_order.lock().expect("call_order lock")[..1],
        ["hydrate"],
        "expected hydrate before first pump"
    );
    Ok(())
}

#[tokio::test]
async fn run_hydrated_interactive_loop_hydrates_exactly_once() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());

    let messages = vec![UiMessageSnapshot::new("user", "history"), {
        let mut s = UiMessageSnapshot::new("assistant", "response");
        s.usage = Some(UiMessageUsageSnapshot {
            input_tokens: None,
            output_tokens: None,
            total_tokens: Some(321),
        });
        s
    }];
    let spawner = ui.make_event_spawner();
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_hydration(messages.clone(), None)
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("interactive loop with hydration: {e}"))?;

    assert_eq!(
        *ui.hydrated_messages.lock().expect("hydrated_messages lock"),
        messages
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_processes_input_while_first_turn_is_running() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["second"],
        &[],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        2,
        Arc::clone(&active_pump_count),
    )
    .with_bus(bus.clone());
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
    let value = result.map_err(|e| format!("interactive loop should stay responsive: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        _runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first", "second"]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_preserves_fifo_for_prompts_queued_while_active() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["second", "third"],
        &[],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        3,
        Arc::clone(&active_pump_count),
    )
    .with_bus(bus.clone());
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
    result.map_err(|e| format!("interactive loop should complete queued prompts: {e}"))?;

    assert_eq!(
        _runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first", "second", "third"]
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn permission_requested_emits_before_execution_and_waits_for_decision_before_side_effects()
-> TResult {
    let bus = create_bus();
    let runtime = PermissionGateRuntime::new().with_bus(bus.clone());
    let pending = runtime.pending.clone();
    let pumps_while_waiting = Arc::new(AtomicUsize::new(0));
    let ui = PermissionOrderingUi::new(
        PermissionDecision::AllowOnce,
        4,
        Arc::clone(&pumps_while_waiting),
    )
    .with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_interactive_pending(Some(pending))
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(_runtime.side_effects.load(Ordering::SeqCst), 1);
    assert!(
        pumps_while_waiting.load(Ordering::SeqCst) > 0,
        "execution must pause while waiting for permission decision"
    );

    let events = ui.events.lock().unwrap();
    let requested_idx = events
        .iter()
        .position(|event| matches!(event, UiEvent::PermissionRequested { .. }))
        .ok_or("PermissionRequested must be emitted")?;
    let tool_start_idx = events
        .iter()
        .position(|event| matches!(event, UiEvent::ToolStarted { .. }))
        .ok_or("ToolStarted should happen after allow decision")?;

    assert!(
        requested_idx < tool_start_idx,
        "PermissionRequested must precede ToolStarted"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn deny_decision_resumes_deterministically_without_pre_decision_handler_side_effects()
-> TResult {
    let bus = create_bus();
    let runtime = PermissionGateRuntime::new().with_bus(bus.clone());
    let pending = runtime.pending.clone();
    let ui = PermissionOrderingUi::new(PermissionDecision::Deny, 3, Arc::new(AtomicUsize::new(0)))
        .with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_interactive_pending(Some(pending))
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(_runtime.side_effects.load(Ordering::SeqCst), 0);

    let events = ui.events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, UiEvent::PermissionRequested { .. }))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, UiEvent::ToolStarted { .. }))
    );
    Ok(())
}

// ── PermissionBridgeUi ──────────────────────────────────────────────────
// UserInputUi-only helper that drives a permission decision through the
// interactive worker bridge, asserting the bus event ordering.

struct PermissionBridgeUi {
    event_tx: mpsc::Sender<OrchestratorEvent>,
    submitted: std::collections::VecDeque<String>,
    pending_decisions: Arc<Mutex<std::collections::VecDeque<PermissionDecisionSubmission>>>,
    events: Arc<Mutex<Vec<crate::bus::PermissionEvent>>>,
    quit: Arc<AtomicBool>,
    decision: PermissionDecision,
    _bus_task: Option<tokio::task::JoinHandle<()>>,
}

impl PermissionBridgeUi {
    fn new(decision: PermissionDecision) -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
        Self {
            event_tx,
            submitted: ["run".to_string()].into_iter().collect(),
            pending_decisions: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            events: Arc::new(Mutex::new(Vec::new())),
            quit: Arc::new(AtomicBool::new(false)),
            decision,
            _bus_task: None,
        }
    }

    fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let pending_decisions = Arc::clone(&self.pending_decisions);
        let events = Arc::clone(&self.events);
        let quit = Arc::clone(&self.quit);
        let decision = self.decision;

        let mut permission_rx = bus.permission().subscribe();
        let mut turn_rx = bus.turn().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = permission_rx.recv() => {
                        events.lock().expect("events lock").push(event.clone());
                        if let crate::bus::PermissionEvent::Requested { ref request_id, ref context } = event {
                            pending_decisions.lock().expect("pending decisions lock").push_back(PermissionDecisionSubmission {
                                request_id: request_id.clone(),
                                decision,
                                matched_rule_identity: context.matched_rule_identity.clone(),
                            });
                        }
                    }
                    Ok(event) = turn_rx.recv() => {
                        if let crate::bus::TurnEvent::Completed { .. } = event {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    else => break,
                }
            }
        }));
        self
    }

    /// Feeds queued inputs into the orchestrator's event channel via
    /// `with_spawn_render_loop`, and terminates when the turn completes.
    fn make_event_spawner(&self) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let submitted = self.submitted.clone();
        let pending_decisions = Arc::clone(&self.pending_decisions);
        let quit = Arc::clone(&self.quit);
        move |event_tx| {
            let event_tx = event_tx.clone();
            let mut submitted = submitted.clone();
            tokio::spawn(async move {
                loop {
                    if let Some(prompt) = submitted.pop_front() {
                        let _ = event_tx
                            .send(OrchestratorEvent::PromptSubmitted { text: prompt })
                            .await;
                    }
                    let decision = pending_decisions.lock().expect("pending lock").pop_front();
                    if let Some(decision) = decision {
                        let _ = event_tx
                            .send(OrchestratorEvent::PermissionDecision { decision })
                            .await;
                    }
                    if quit.load(Ordering::SeqCst) {
                        let _ = event_tx.send(OrchestratorEvent::Quit).await;
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            });
        }
    }
}

impl UserInputUi for PermissionBridgeUi {
    fn event_sender(&self) -> &mpsc::Sender<OrchestratorEvent> {
        &self.event_tx
    }
}

#[tokio::test]
#[serial_test::serial]
async fn permission_flow_reaches_bus_through_worker_bridge() -> TResult {
    let bus = create_bus();
    let runtime = PermissionGateRuntime::new().with_bus(bus.clone());
    let pending = runtime.pending.clone();
    let ui = PermissionBridgeUi::new(PermissionDecision::AllowOnce).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_interactive_pending(Some(pending))
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.side_effects.load(Ordering::SeqCst), 1);

    let events = ui.events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, crate::bus::PermissionEvent::Requested { .. })),
        "permission resolver must publish a PermissionEvent::Requested to the permission bus"
    );
    Ok(())
}

// ── PermissionTimeoutIgnoredRuntime ───────────────────────────────────

struct PermissionTimeoutIgnoredRuntime {
    request_id: String,
    bus: crate::bus::Bus,
}

impl CoreRuntime for PermissionTimeoutIgnoredRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        let _ = self
            .bus
            .permission()
            .send(crate::bus::PermissionEvent::DecisionTimedOut {
                request_id: self.request_id.clone(),
            })
            .await;
        let _ = self
            .bus
            .permission()
            .send(crate::bus::PermissionEvent::DecisionIgnored {
                request_id: self.request_id.clone(),
                reason: "decision_channel_closed".to_string(),
            })
            .await;
        // Publish TurnCompleted directly to the bus (worker bridge no longer
        // converts UiEvent::Completed), matching production.
        let _ = self
            .bus
            .turn()
            .send(crate::bus::TurnEvent::Completed { tool_calls: 0 })
            .await;
        Ok(Value::nothing(span))
    }
}

impl McpManagement for PermissionTimeoutIgnoredRuntime {
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

crate::default_session!(PermissionTimeoutIgnoredRuntime);

impl ModelSwitching for PermissionTimeoutIgnoredRuntime {
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

#[tokio::test]
#[serial_test::serial]
async fn permission_timeout_and_ignored_reach_bus_through_worker_bridge() -> TResult {
    let bus = create_bus();
    let runtime = PermissionTimeoutIgnoredRuntime {
        request_id: "ask-0000000000000abc".to_string(),
        bus: bus.clone(),
    };
    let ui = PermissionBridgeUi::new(PermissionDecision::AllowOnce).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;
    assert!(value.is_nothing());

    let events = ui.events.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, crate::bus::PermissionEvent::DecisionTimedOut { .. })),
        "worker bridge must forward PermissionEvent::DecisionTimedOut to the permission bus"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, crate::bus::PermissionEvent::DecisionIgnored { .. })),
        "worker bridge must forward PermissionEvent::DecisionIgnored to the permission bus"
    );
    Ok(())
}

#[tokio::test]
async fn models_launcher_opens_picker_while_worker_active() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let ui = ModelPickerLaunchWhileActiveUi::new(&["first"], 1).with_bus(bus.clone());
    let spawner = ui.make_event_spawner(bus.clone());

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
    let value = result
        .map_err(|e| format!("interactive loop should process model launcher while active: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        ui.shared_actions.lock().unwrap().clone(),
        vec![SharedUiAction::Models]
    );
    assert_eq!(
        _runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
    Ok(())
}

#[tokio::test]
async fn models_slash_opens_picker_while_worker_active() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let ui = ModelPickerLaunchWhileActiveUi::new(&["first"], 1).with_bus(bus.clone());
    let spawner = ui.make_event_spawner(bus.clone());

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
    let value =
        result.map_err(|e| format!("interactive loop should process /models while active: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        ui.shared_actions.lock().unwrap().clone(),
        vec![SharedUiAction::Models]
    );
    assert_eq!(
        _runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_global_abort_cancels_active_and_does_not_run_queued_prompt() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let ui = FakeInteractiveUi::with_prompts(&["first"])
        .with_min_bus_events(3)
        .with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    // Spawn a background task that unblocks the turn after a short delay.
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        unblock.store(true, Ordering::SeqCst);
    });

    let (rt, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result
        .map_err(|e| format!("interactive loop should treat cancellation as non-fatal: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        rt.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_startup_hydration_initializes_per_server_visible_counts_before_toggles()
-> TResult {
    let bus = create_bus();
    let runtime = StartupHydrationRuntime {
        names_by_server: vec![
            (
                "gh".to_string(),
                vec!["gh__issues".to_string(), "gh__prs".to_string()],
            ),
            ("k8s".to_string(), vec!["k8s__pods".to_string()]),
        ],
        bus: bus.clone(),
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        ui.mcp_visible_tool_count_updates.lock().unwrap().clone(),
        vec![("gh".to_string(), 2), ("k8s".to_string(), 1)]
    );
    Ok(())
}

#[test]
fn emit_batch_delivers_all_events() {
    // RED phase: write test that verifies emit_batch delivers all events
    struct BatchTestUi {
        event_tx: mpsc::Sender<OrchestratorEvent>,
        events: Vec<UiEvent>,
        emit_calls: usize,
        emit_batch_calls: usize,
    }

    impl Default for BatchTestUi {
        fn default() -> Self {
            let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
            Self {
                event_tx,
                events: Vec::new(),
                emit_calls: 0,
                emit_batch_calls: 0,
            }
        }
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

    impl UserInputUi for BatchTestUi {
        fn event_sender(&self) -> &mpsc::Sender<OrchestratorEvent> {
            &self.event_tx
        }
    }

    let mut ui = BatchTestUi::default();

    // Create 5 test events
    let events = vec![
        UiEvent::Tick,
        UiEvent::LlmStarted,
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

// ── McpToggleRuntime ────────────────────────────────────────────────────

struct McpToggleRuntime {
    toggles: Vec<(String, bool)>,
    next_state: McpUsabilityState,
    visible_count: usize,
    visible_count_by_server: usize,
    bus: crate::bus::Bus,
}

impl CoreRuntime for McpToggleRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        let _ = self
            .bus
            .turn()
            .send(crate::bus::TurnEvent::Completed { tool_calls: 0 })
            .await;
        Ok(Value::nothing(span))
    }
}

impl McpManagement for McpToggleRuntime {
    async fn set_mcp_server_enabled(
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

impl ModelSwitching for McpToggleRuntime {
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

// ── FailingMcpToggleRuntime ─────────────────────────────────────────────

struct FailingMcpToggleRuntime {
    toggles: Vec<(String, bool)>,
    visible_count: usize,
    bus: crate::bus::Bus,
}

impl CoreRuntime for FailingMcpToggleRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        let _ = self
            .bus
            .turn()
            .send(crate::bus::TurnEvent::Completed { tool_calls: 0 })
            .await;
        Ok(Value::nothing(span))
    }
}

impl McpManagement for FailingMcpToggleRuntime {
    async fn set_mcp_server_enabled(
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

impl ModelSwitching for FailingMcpToggleRuntime {
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

// ── SequencedMcpToggleRuntime ───────────────────────────────────────────

struct SequencedMcpToggleRuntime {
    toggles: Vec<(String, bool)>,
    states: std::collections::VecDeque<McpUsabilityState>,
    visible_counts: std::collections::VecDeque<usize>,
    visible_counts_by_server: std::collections::VecDeque<usize>,
    current_visible_count: usize,
    current_visible_count_by_server: usize,
    bus: crate::bus::Bus,
}

impl CoreRuntime for SequencedMcpToggleRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        let _ = self
            .bus
            .turn()
            .send(crate::bus::TurnEvent::Completed { tool_calls: 0 })
            .await;
        Ok(Value::nothing(span))
    }
}

impl McpManagement for SequencedMcpToggleRuntime {
    async fn set_mcp_server_enabled(
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

impl ModelSwitching for SequencedMcpToggleRuntime {
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

// ── StagedToggleUi ──────────────────────────────────────────────────────

struct StagedToggleUi {
    event_tx: mpsc::Sender<OrchestratorEvent>,
    quit: Arc<AtomicBool>,
    first_response_processed: Arc<AtomicBool>,
    second_response_processed: Arc<AtomicBool>,
    _ui_state_task: Option<tokio::task::JoinHandle<()>>,
}

impl StagedToggleUi {
    fn new() -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
        Self {
            event_tx,
            quit: Arc::new(AtomicBool::new(false)),
            first_response_processed: Arc::new(AtomicBool::new(false)),
            second_response_processed: Arc::new(AtomicBool::new(false)),
            _ui_state_task: None,
        }
    }

    fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let first_response_processed = Arc::clone(&self.first_response_processed);
        let second_response_processed = Arc::clone(&self.second_response_processed);
        let quit = Arc::clone(&self.quit);
        let mut rx = bus.ui_state().subscribe();
        self._ui_state_task = Some(tokio::spawn(async move {
            while let Ok(event) = rx.recv().await {
                if matches!(
                    event,
                    UiStateEvent::SetMcpServerState { server: ref s, .. }
                        if s == "gh"
                ) {
                    let was_first = first_response_processed
                        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok();
                    if !was_first {
                        second_response_processed.store(true, Ordering::SeqCst);
                        quit.store(true, Ordering::SeqCst);
                    }
                }
            }
        }));
        self
    }
}

impl UserInputUi for StagedToggleUi {
    fn event_sender(&self) -> &mpsc::Sender<OrchestratorEvent> {
        &self.event_tx
    }
}

impl StagedToggleUi {
    /// Feeds the two sequenced MCP toggles: the first immediately, the second
    /// only after the first response's `SetMcpServerState` has been observed.
    /// Sends `Quit` once both responses have been processed.
    fn make_event_spawner(&self) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let first_response_processed = Arc::clone(&self.first_response_processed);
        let second_response_processed = Arc::clone(&self.second_response_processed);
        move |event_tx| {
            let event_tx = event_tx.clone();
            tokio::spawn(async move {
                let _ = event_tx
                    .send(OrchestratorEvent::UiRequest(UiRequest::ToggleMcp {
                        server: "gh".to_string(),
                        enable: false,
                    }))
                    .await;
                while !first_response_processed.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                let _ = event_tx
                    .send(OrchestratorEvent::UiRequest(UiRequest::ToggleMcp {
                        server: "gh".to_string(),
                        enable: true,
                    }))
                    .await;
                while !second_response_processed.load(Ordering::SeqCst) {
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                let _ = event_tx.send(OrchestratorEvent::Quit).await;
            });
        }
    }
}

#[tokio::test]
async fn interactive_loop_processes_mcp_toggle_requests_and_updates_ui_state() -> TResult {
    let bus = create_bus();
    let runtime = McpToggleRuntime {
        toggles: Vec::new(),
        next_state: McpUsabilityState::Disabled,
        visible_count: 3,
        visible_count_by_server: 0,
        bus: bus.clone(),
    };
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_expected_mcp_updates(1)
        .with_bus(bus.clone());
    ui.mcp_toggle_requests
        .lock()
        .unwrap()
        .push_back(McpToggleRequest {
            server_name: "gh".to_string(),
            enable: false,
        });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), false)]);
    assert_eq!(
        ui.mcp_details.lock().unwrap().clone(),
        vec![("gh".to_string(), McpUsabilityState::Disabled, None, 3)]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates.lock().unwrap().clone(),
        vec![("gh".to_string(), 0)]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_marks_enable_failure_as_failed_state() -> TResult {
    let bus = create_bus();
    let runtime = McpToggleRuntime {
        toggles: Vec::new(),
        next_state: McpUsabilityState::Failed,
        visible_count: 2,
        visible_count_by_server: 0,
        bus: bus.clone(),
    };
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_expected_mcp_updates(1)
        .with_bus(bus.clone());
    ui.mcp_toggle_requests
        .lock()
        .unwrap()
        .push_back(McpToggleRequest {
            server_name: "gh".to_string(),
            enable: true,
        });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), true)]);
    assert_eq!(
        ui.mcp_details.lock().unwrap().clone(),
        vec![("gh".to_string(), McpUsabilityState::Failed, None, 2)]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates.lock().unwrap().clone(),
        vec![("gh".to_string(), 0)]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_marks_enable_success_as_enabled_state() -> TResult {
    let bus = create_bus();
    let runtime = McpToggleRuntime {
        toggles: Vec::new(),
        next_state: McpUsabilityState::Enabled,
        visible_count: 7,
        visible_count_by_server: 5,
        bus: bus.clone(),
    };
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_expected_mcp_updates(1)
        .with_bus(bus.clone());
    ui.mcp_toggle_requests
        .lock()
        .unwrap()
        .push_back(McpToggleRequest {
            server_name: "gh".to_string(),
            enable: true,
        });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), true)]);
    assert_eq!(
        ui.mcp_details.lock().unwrap().clone(),
        vec![("gh".to_string(), McpUsabilityState::Enabled, None, 7)]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates.lock().unwrap().clone(),
        vec![("gh".to_string(), 5)]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_loop_propagates_failure_reason_and_visible_tool_count_on_toggle_error()
-> TResult {
    let bus = create_bus();
    let runtime = FailingMcpToggleRuntime {
        toggles: Vec::new(),
        visible_count: 4,
        bus: bus.clone(),
    };
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_expected_mcp_updates(1)
        .with_bus(bus.clone());
    ui.mcp_toggle_requests
        .lock()
        .unwrap()
        .push_back(McpToggleRequest {
            server_name: "gh".to_string(),
            enable: true,
        });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.toggles, vec![("gh".to_string(), true)]);
    assert_eq!(
        ui.mcp_details.lock().unwrap().clone(),
        vec![(
            "gh".to_string(),
            McpUsabilityState::Failed,
            Some("connect timeout".to_string()),
            4,
        )]
    );
    assert_eq!(
        ui.mcp_visible_tool_count_updates.lock().unwrap().clone(),
        vec![("gh".to_string(), 0)]
    );
    Ok(())
}

#[tokio::test]
async fn interactive_toggle_enable_disable_cycle_refreshes_per_server_visible_counts() -> TResult {
    let bus = create_bus();
    let runtime = SequencedMcpToggleRuntime {
        toggles: Vec::new(),
        states: [McpUsabilityState::Disabled, McpUsabilityState::Enabled]
            .into_iter()
            .collect(),
        visible_counts: [3usize, 7usize].into_iter().collect(),
        visible_counts_by_server: [0usize, 5usize].into_iter().collect(),
        current_visible_count: 0,
        current_visible_count_by_server: 0,
        bus: bus.clone(),
    };
    let ui = StagedToggleUi::new().with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.toggles,
        vec![("gh".to_string(), false), ("gh".to_string(), true)]
    );
    Ok(())
}

#[tokio::test]
async fn palette_models_does_not_bypass_shared_models_action_path() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["/models"]).with_bus(bus.clone());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        ui.shared_actions.lock().unwrap().clone(),
        vec![SharedUiAction::Models]
    );
    assert!(runtime.prompts.is_empty());
    Ok(())
}

#[tokio::test]
async fn inline_model_picker_enter_switches_active_model_and_provider() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[])
        .with_min_bus_events(2)
        .with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap()
        .push_back("openai/gpt-4o-mini".to_string());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    assert_eq!(
        *ui.active_model_identity.lock().unwrap(),
        Some("openai/gpt-4o-mini".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn model_switch_failure_keeps_previous_model_and_warns() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        switch_model_result: Some(Err("switch failed".to_string())),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap()
        .push_back("openai/gpt-4o-mini".to_string());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    assert_eq!(
        *ui.active_model_identity.lock().unwrap(),
        Some("openai/gpt-4o-mini".to_string())
    );
    assert!(
        ui.warnings
            .lock()
            .unwrap()
            .iter()
            .any(|w| w == "switch failed")
    );
    Ok(())
}

#[tokio::test]
async fn model_switch_uses_cached_startup_plugin_config() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap()
        .push_back("openai/gpt-4o-mini".to_string());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    Ok(())
}

#[tokio::test]
async fn model_switch_updates_footer_active_model_identity_immediately() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap()
        .push_back("openai/gpt-4o-mini".to_string());

    let spawner = ui.make_event_spawner();
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        *ui.active_model_identity.lock().unwrap(),
        Some("openai/gpt-4o-mini".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn model_switch_result_artifact_is_rendered() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap()
        .push_back("openai/gpt-4o-mini".to_string());

    let spawner = ui.make_event_spawner();
    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert!(
        !ui.warnings
            .lock()
            .unwrap()
            .iter()
            .any(|w| w.starts_with("Model switched"))
    );
    Ok(())
}

#[tokio::test]
async fn next_turn_uses_newly_selected_model() -> TResult {
    let bus = create_bus();
    let runtime = FakeRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = FakeInteractiveUi::with_prompts(&["after-switch"]).with_bus(bus.clone());
    ui.model_switch_requests
        .lock()
        .unwrap()
        .push_back("openai/gpt-4o-mini".to_string());

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        runtime.switched_models,
        vec!["openai/gpt-4o-mini".to_string()]
    );
    assert_eq!(runtime.prompts, vec!["after-switch".to_string()]);
    Ok(())
}

#[tokio::test]
async fn model_switch_while_worker_active_is_queued_for_next_turn() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let ui = ResponsiveInteractiveUi::new(
        &["first"],
        &[],
        &["openai/gpt-4o-mini"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        1,
        Arc::clone(&active_pump_count),
    )
    .with_bus(bus.clone());
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
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        _runtime.prompts.lock().expect("prompts lock").as_slice(),
        ["first"]
    );
    assert_eq!(
        _runtime
            .switched_models
            .lock()
            .expect("switched models lock")
            .as_slice(),
        ["openai/gpt-4o-mini"]
    );
    Ok(())
}

#[tokio::test]
async fn queued_model_switch_applies_after_current_turn_before_next_dispatch() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let ui = ResponsiveInteractiveUi::new(
        &["first"],
        &["second"],
        &["openai/gpt-4o-mini"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        2,
        Arc::clone(&active_pump_count),
    )
    .with_bus(bus.clone());
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
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        _runtime
            .action_log
            .lock()
            .expect("action log lock")
            .as_slice(),
        ["turn:first", "turn:second", "switch:openai/gpt-4o-mini"]
    );
    Ok(())
}

#[tokio::test]
async fn queued_model_switch_last_write_wins() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let ui = ResponsiveInteractiveUi::new(
        &["first"],
        &[],
        &["openai/gpt-4o-mini", "anthropic/claude-3-5-sonnet"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        1,
        Arc::clone(&active_pump_count),
    )
    .with_bus(bus.clone());
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
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        _runtime
            .switched_models
            .lock()
            .expect("switched models lock")
            .as_slice(),
        ["anthropic/claude-3-5-sonnet"]
    );
    Ok(())
}

#[tokio::test]
async fn queued_model_switch_failure_keeps_previous_model_and_warns() -> TResult {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn))
        .with_switch_model_result(Err("queued switch failed".to_string()))
        .with_bus(bus.clone());
    let active_pump_count = Arc::new(AtomicUsize::new(0));
    let ui = ResponsiveInteractiveUi::new(
        &["first"],
        &[],
        &["openai/gpt-4o-mini"],
        Arc::clone(&runtime.active),
        Arc::clone(&block_first_turn),
        1,
        Arc::clone(&active_pump_count),
    )
    .with_bus(bus.clone());
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
    let value = result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        _runtime.active_model_identity(),
        "openai/gpt-4o-mini",
        "failed queued switch must keep previous active identity"
    );
    assert!(
        ui.warnings
            .lock()
            .unwrap()
            .iter()
            .any(|w| w == "queued switch failed")
    );
    Ok(())
}

// ── CancellableBlockingRuntime ─────────────────────────────────────────
// Blocks in execute_turn until the block flag is unset OR a cancellation is
// requested via the ProgressUi. Used to verify that an A2A task cancel stops
// the running turn.

#[derive(Clone)]
struct CancellableBlockingRuntime {
    block: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    started: Arc<AtomicBool>,
    prompts: Arc<Mutex<Vec<String>>>,
    bus: crate::bus::Bus,
}

impl CancellableBlockingRuntime {
    fn new(block: Arc<AtomicBool>) -> Self {
        Self {
            block,
            cancelled: Arc::new(AtomicBool::new(false)),
            started: Arc::new(AtomicBool::new(false)),
            prompts: Arc::new(Mutex::new(Vec::new())),
            bus: crate::bus::Bus::default(),
        }
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        self.bus = bus;
        self
    }
}

impl CoreRuntime for CancellableBlockingRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.lock().expect("prompts lock").push(prompt);
        self.started.store(true, Ordering::SeqCst);
        // Subscribe to the bus cancel channel directly, matching production where
        // the hook/tool proxies subscribe to `bus.cancel()`.
        let mut cancel_rx = self.bus.cancel().subscribe();
        loop {
            tokio::select! {
                recv = cancel_rx.recv() => {
                    if matches!(recv, Ok(crate::bus::CancelEvent::Requested) | Err(crate::bus::ChannelError::Lagged { .. })) {
                        self.cancelled.store(true, Ordering::SeqCst);
                        let _ = self
                            .bus
                            .turn()
                            .send(crate::bus::TurnEvent::Completed { tool_calls: 0 })
                            .await;
                        return Err(LabeledError::new("LLM call cancelled"));
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(2)) => {
                    if self.block.load(Ordering::SeqCst) {
                        break;
                    }
                }
            }
        }
        let _ = self
            .bus
            .turn()
            .send(crate::bus::TurnEvent::Completed { tool_calls: 0 })
            .await;
        Ok(Value::nothing(Span::test_data()))
    }
}

impl ModelSwitching for CancellableBlockingRuntime {
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

crate::default_session!(CancellableBlockingRuntime);
crate::default_mcp!(CancellableBlockingRuntime);

#[tokio::test]
async fn matching_a2a_task_cancel_sets_cancel_requested() -> TResult {
    let block = Arc::new(AtomicBool::new(false));
    let bus = create_bus();
    let runtime = CancellableBlockingRuntime::new(Arc::clone(&block)).with_bus(bus.clone());
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());

    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Publish an external A2A task, retrying until the loop subscribes.
    let publish_bus = bus.clone();
    tokio::spawn(async move {
        let event = ExternalEvent::PromptReceived {
            prompt: "[A2A Task 11111111-2222-3333-4444-555555555555 from http://a.local]: do work"
                .to_string(),
            task_id: "11111111-2222-3333-4444-555555555555".to_string(),
        };
        while publish_bus.external().send(event.clone()).await.is_err() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });

    // Send a matching cancel once the turn has started.
    let started = runtime.started.clone();
    let cancel_tx_clone = cancel_tx.clone();
    tokio::spawn(async move {
        while !started.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        cancel_tx_clone
            .send("11111111-2222-3333-4444-555555555555".to_string())
            .unwrap();
    });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_task_cancel_rx(Some(cancel_rx))
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(
        runtime.cancelled(),
        "matching A2A task cancel must set cancel_requested and stop the turn"
    );
    // Unblock so the test exits cleanly.
    block.store(true, Ordering::SeqCst);
    Ok(())
}

#[tokio::test]
async fn non_matching_a2a_task_cancel_does_not_set_cancel_requested() -> TResult {
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let bus = create_bus();
    let runtime = LongRunningRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());

    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Publish an external A2A task, retrying until the loop subscribes.
    let publish_bus = bus.clone();
    tokio::spawn(async move {
        let event = ExternalEvent::PromptReceived {
            prompt: "[A2A Task 11111111-2222-3333-4444-555555555555 from http://a.local]: do work"
                .to_string(),
            task_id: "11111111-2222-3333-4444-555555555555".to_string(),
        };
        while publish_bus.external().send(event.clone()).await.is_err() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });
    cancel_tx
        .send("99999999-9999-9999-9999-999999999999".to_string())
        .unwrap();

    // Unblock the turn after a delay; a non-matching cancel must NOT cancel it.
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        unblock.store(true, Ordering::SeqCst);
    });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_task_cancel_rx(Some(cancel_rx))
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("interactive loop: {e}"))?;

    let prompts = runtime.prompts.lock().expect("prompts lock");
    assert_eq!(prompts.len(), 1, "one external prompt should be processed");
    assert!(
        prompts[0].starts_with("[A2A Task 11111111-2222-3333-4444-555555555555"),
        "non-matching cancel must not abort the running turn"
    );
    Ok(())
}

#[tokio::test]
async fn matching_a2a_task_cancel_stops_running_turn() -> TResult {
    let block = Arc::new(AtomicBool::new(false));
    let bus = create_bus();
    let runtime = CancellableBlockingRuntime::new(Arc::clone(&block)).with_bus(bus.clone());
    let ui = FakeInteractiveUi::with_prompts(&[]).with_bus(bus.clone());

    let (cancel_tx, cancel_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Publish an external A2A task that blocks, then send a matching cancel
    // shortly after the turn has started.
    let publish_bus = bus.clone();
    tokio::spawn(async move {
        let event = ExternalEvent::PromptReceived {
            prompt: "[A2A Task 22222222-3333-4444-5555-666666666666 from http://a.local]: do work"
                .to_string(),
            task_id: "22222222-3333-4444-5555-666666666666".to_string(),
        };
        while publish_bus.external().send(event.clone()).await.is_err() {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    });

    let started = runtime.started.clone();
    let cancel_tx_clone = cancel_tx.clone();
    tokio::spawn(async move {
        // Wait until the turn has started, then cancel the matching task.
        while !started.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        cancel_tx_clone
            .send("22222222-3333-4444-5555-666666666666".to_string())
            .unwrap();
    });

    let spawner = ui.make_event_spawner();
    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_task_cancel_rx(Some(cancel_rx))
            .with_spawn_render_loop(spawner),
    )
    .await;
    result.map_err(|e| format!("interactive loop: {e}"))?;

    assert!(
        runtime.cancelled(),
        "matching A2A task cancel must stop the running turn"
    );
    // Unblock so the test exits cleanly.
    block.store(true, Ordering::SeqCst);
    Ok(())
}
