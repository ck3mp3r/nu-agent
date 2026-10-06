use super::*;

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
            .ui_event()
            .send(UiEvent::Completed { tool_calls: 0 })
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
            .ui_event()
            .send(UiEvent::Completed { tool_calls: 0 })
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
            .ui_event()
            .send(UiEvent::Completed { tool_calls: 0 })
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
