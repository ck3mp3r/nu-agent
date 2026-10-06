use super::*;

// ── PermissionGateRuntime ───────────────────────────────────────────────

pub(super) struct PermissionGateRuntime {
    pub(super) side_effects: Arc<AtomicUsize>,
    requested: Arc<AtomicBool>,
    finished: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
    request_id: String,
    rule_identity: String,
    pub(super) pending: crate::conversation::runtime::PendingPermissions,
    bus: crate::bus::Bus,
}

impl PermissionGateRuntime {
    pub(super) fn new() -> Self {
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

    pub(super) fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
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
            tool_key: "nu\n{\"command\":\"echo hi\"}".to_string(),
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
            .ui_event()
            .send(UiEvent::PermissionRequested {
                request_id: request_id.clone(),
                context,
            })
            .await;
        self.requested.store(true, Ordering::SeqCst);

        // Await the UI's decision via the oneshot. Deny on channel drop.
        let decision = rx.await.unwrap_or(PermissionDecision::Deny);

        if decision != PermissionDecision::Deny {
            self.side_effects.fetch_add(1, Ordering::SeqCst);
            // Publish the tool-start to the bus ui_event channel directly,
            // matching production where the hook publishes UiEvent::ToolStarted.
            let _ = self
                .bus
                .ui_event()
                .send(UiEvent::ToolStarted {
                    name: "nu".to_string(),
                    source: "closure".to_string(),
                    arguments: r#"{"command":"echo hi"}"#.to_string(),
                    call_line: CallLine::from_json_summary(r#"{"command":"echo hi"}"#),
                })
                .await;
        }

        // Publish TurnCompleted directly to the bus (worker bridge no longer
        // converts UiEvent::Completed), matching production.
        let _ = self
            .bus
            .ui_event()
            .send(UiEvent::Completed { tool_calls: 0 })
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

        let mut ui_event_rx = bus.ui_event().subscribe();

        self._background_tasks.push(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = ui_event_rx.recv() => {
                        match event {
                            UiEvent::PermissionRequested { request_id, context } => {
                                request_seen.store(true, Ordering::SeqCst);
                                pending_decisions.lock().expect("pending decisions lock").push_back(PermissionDecisionSubmission {
                                    request_id: request_id.clone(),
                                    decision,
                                    matched_rule_identity: context.matched_rule_identity.clone(),
                                });
                                events.lock().expect("events lock").push(UiEvent::PermissionRequested {
                                    request_id,
                                    context,
                                });
                            }
                            UiEvent::PermissionDecisionTimedOut { request_id } => {
                                events.lock().expect("events lock").push(UiEvent::PermissionDecisionTimedOut { request_id });
                            }
                            UiEvent::PermissionDecisionIgnored { request_id, reason } => {
                                events.lock().expect("events lock").push(UiEvent::PermissionDecisionIgnored { request_id, reason });
                            }
                            UiEvent::ToolStarted { name, source, arguments, call_line } => {
                                events.lock().expect("events lock").push(UiEvent::ToolStarted {
                                    name,
                                    source,
                                    arguments,
                                    call_line,
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

pub(super) struct ModelPickerLaunchWhileActiveUi {
    event_tx: mpsc::Sender<OrchestratorEvent>,
    submitted: std::collections::VecDeque<String>,
    pending_model_picker_launch_requests: usize,
    quit: Arc<AtomicBool>,
    completed_count: Arc<AtomicUsize>,
    expected_completions: usize,
    pub(super) shared_actions: Arc<Mutex<Vec<SharedUiAction>>>,
    _bus_task: Option<tokio::task::JoinHandle<()>>,
}

impl ModelPickerLaunchWhileActiveUi {
    pub(super) fn new(initial_prompts: &[&str], expected_completions: usize) -> Self {
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

    pub(super) fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let quit = Arc::clone(&self.quit);
        let completed_count = Arc::clone(&self.completed_count);
        let shared_actions = Arc::clone(&self.shared_actions);
        let expected_completions = self.expected_completions;

        let mut ui_event_rx = bus.ui_event().subscribe();
        let mut ui_state_rx = bus.ui_state().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = ui_event_rx.recv() => {
                        if let UiEvent::Completed { .. } = event {
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

    pub(super) fn make_event_spawner(
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

#[tokio::test]
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
