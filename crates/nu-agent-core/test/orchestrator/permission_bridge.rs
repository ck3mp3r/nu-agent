use super::permission_flow::{ModelPickerLaunchWhileActiveUi, PermissionGateRuntime};
use super::*;

// ── PermissionBridgeUi ──────────────────────────────────────────────────
// UserInputUi-only helper that drives a permission decision through the
// interactive worker bridge, asserting the bus event ordering.

struct PermissionBridgeUi {
    event_tx: mpsc::Sender<OrchestratorEvent>,
    submitted: std::collections::VecDeque<String>,
    pending_decisions: Arc<Mutex<std::collections::VecDeque<PermissionDecisionSubmission>>>,
    events: Arc<Mutex<Vec<UiEvent>>>,
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

        let mut ui_event_rx = bus.ui_event().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = ui_event_rx.recv() => {
                        events.lock().expect("events lock").push(event.clone());
                        match event {
                            UiEvent::PermissionRequested { request_id, context } => {
                                pending_decisions.lock().expect("pending decisions lock").push_back(PermissionDecisionSubmission {
                                    request_id,
                                    decision,
                                    matched_rule_identity: context.matched_rule_identity.clone(),
                                });
                            }
                            UiEvent::Completed { .. } => {
                                quit.store(true, Ordering::SeqCst);
                            }
                            _ => {}
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
            .any(|event| matches!(event, UiEvent::PermissionRequested { .. })),
        "permission resolver must publish a UiEvent::PermissionRequested to the ui_event bus"
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
            .ui_event()
            .send(UiEvent::PermissionDecisionTimedOut {
                request_id: self.request_id.clone(),
            })
            .await;
        let _ = self
            .bus
            .ui_event()
            .send(UiEvent::PermissionDecisionIgnored {
                request_id: self.request_id.clone(),
                reason: "decision_channel_closed".to_string(),
            })
            .await;
        // Publish TurnCompleted directly to the bus (worker bridge no longer
        // converts UiEvent::Completed), matching production.
        let _ = self
            .bus
            .ui_event()
            .send(UiEvent::Completed { tool_calls: 0 })
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
            .any(|event| matches!(event, UiEvent::PermissionDecisionTimedOut { .. })),
        "worker bridge must forward UiEvent::PermissionDecisionTimedOut to the ui_event bus"
    );
    assert!(
        events
            .iter()
            .any(|event| matches!(event, UiEvent::PermissionDecisionIgnored { .. })),
        "worker bridge must forward UiEvent::PermissionDecisionIgnored to the ui_event bus"
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
