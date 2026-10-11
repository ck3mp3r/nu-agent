use super::*;

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
            .ui_event()
            .send(UiEvent::Completed { tool_calls: 0 })
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

        let mut ui_event_rx = bus.ui_event().subscribe();
        let mut ui_state_rx = bus.ui_state().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = ui_event_rx.recv() => {
                        match event {
                            UiEvent::Warning { message } | UiEvent::TurnError { message } => {
                                warnings.lock().expect("warnings lock").push(message);
                            }
                            _ => {}
                        }
                    }
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
            .ui_event()
            .send(UiEvent::Completed { tool_calls: 0 })
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
