use super::*;

// ---------------------------------------------------------------------------
// Long-running runtime with agent switch support (for queuing tests)
// ---------------------------------------------------------------------------

struct LongRunningAgentRuntime {
    prompts: Arc<Mutex<Vec<String>>>,
    switched_agents: Arc<Mutex<Vec<String>>>,
    active: Arc<AtomicBool>,
    block_first_turn: Arc<AtomicBool>,
    active_model: String,
    bus: crate::bus::Bus,
    turn_started_tx: tokio::sync::watch::Sender<bool>,
}

impl LongRunningAgentRuntime {
    fn new(block_first_turn: Arc<AtomicBool>) -> Self {
        let (turn_started_tx, _) = tokio::sync::watch::channel(false);
        Self {
            prompts: Arc::new(Mutex::new(Vec::new())),
            switched_agents: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(AtomicBool::new(false)),
            block_first_turn,
            active_model: "openai/gpt-4o-mini".to_string(),
            bus: crate::bus::Bus::default(),
            turn_started_tx,
        }
    }

    /// Signals when the blocking `first` turn has begun. Tests await this
    /// instead of sleeping before they unblock the turn.
    fn turn_started_rx(&self) -> tokio::sync::watch::Receiver<bool> {
        self.turn_started_tx.subscribe()
    }

    fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        self.bus = bus;
        self
    }
}

impl CoreRuntime for LongRunningAgentRuntime {
    async fn execute_turn(
        &mut self,
        bus: &crate::bus::Bus,
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
            // Signal that the blocking turn has begun so tests can unblock it
            // without sleeping.
            let _ = self.turn_started_tx.send(true);
            let mut cancel_rx = bus.cancel().subscribe();
            loop {
                tokio::select! {
                    Ok(crate::bus::CancelEvent::Requested) = cancel_rx.recv() => {
                        self.active.store(false, Ordering::SeqCst);
                        return Err(LabeledError::new("LLM call cancelled"));
                    }
                    _ = tokio::time::sleep(Duration::from_millis(2)) => {
                        if self.block_first_turn.load(Ordering::SeqCst) {
                            break;
                        }
                    }
                }
            }
        }

        // Publish TurnCompleted directly to the bus (worker bridge no longer
        // converts UiEvent::Completed), matching production.
        let _ = self
            .bus
            .ui_event()
            .send(UiEvent::Completed {
                tool_calls: 0,
                input_tokens: 0,
                output_tokens: 0,
                total_tokens: None,
                cost: None,
                cost_complete: true,
                duration_ms: 0,
            })
            .await;
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
    async fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    async fn switch_agent(&mut self, agent_name: &str) -> Result<String, String> {
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

// ---------------------------------------------------------------------------
// Responsive interactive UI that injects agent switch requests while busy
// ---------------------------------------------------------------------------

struct ResponsiveAgentSwitchUi {
    event_tx: tokio::sync::mpsc::Sender<OrchestratorEvent>,
    submitted: std::collections::VecDeque<String>,
    agent_switch_requests: std::collections::VecDeque<String>,
    quit: Arc<AtomicBool>,
    completed_count: Arc<Mutex<usize>>,
    expected_completions: usize,
    warnings: Arc<Mutex<Vec<String>>>,
    active_agent_identity: Arc<Mutex<Option<String>>>,
    drained_tx: Arc<tokio::sync::watch::Sender<bool>>,
    _turn_task: Option<tokio::task::JoinHandle<()>>,
}

impl ResponsiveAgentSwitchUi {
    fn new(
        initial_prompts: &[&str],
        injected_agent_switch_during_active: &[&str],
        expected_completions: usize,
    ) -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
        let (drained_tx, _) = tokio::sync::watch::channel(false);
        Self {
            event_tx,
            submitted: initial_prompts.iter().map(|s| s.to_string()).collect(),
            agent_switch_requests: injected_agent_switch_during_active
                .iter()
                .map(|s| s.to_string())
                .collect(),
            quit: Arc::new(AtomicBool::new(false)),
            completed_count: Arc::new(Mutex::new(0)),
            expected_completions,
            warnings: Arc::new(Mutex::new(Vec::new())),
            active_agent_identity: Arc::new(Mutex::new(None)),
            drained_tx: Arc::new(drained_tx),
            _turn_task: None,
        }
    }

    /// Signals when the spawner has injected every queued agent switch. Tests
    /// await this before unblocking the turn, so the injections settle without
    /// a fixed sleep.
    fn drained_rx(&self) -> tokio::sync::watch::Receiver<bool> {
        self.drained_tx.subscribe()
    }

    fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let quit = Arc::clone(&self.quit);
        let completed_count = Arc::clone(&self.completed_count);
        let expected_completions = self.expected_completions;
        let warnings = Arc::clone(&self.warnings);
        let active_agent_identity = Arc::clone(&self.active_agent_identity);

        let mut ui_event_rx = bus.ui_event().subscribe();
        let mut ui_state_rx = bus.ui_state().subscribe();

        self._turn_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = ui_event_rx.recv() => {
                        match event {
                            UiEvent::Completed { .. } => {
                                let count = {
                                    let mut count = completed_count.lock().expect("completed count lock");
                                    *count += 1;
                                    *count
                                };
                                if count >= expected_completions {
                                    quit.store(true, Ordering::SeqCst);
                                }
                            }
                            UiEvent::Warning { message } | UiEvent::TurnError { message } => {
                                warnings.lock().expect("warnings lock").push(message);
                            }
                            _ => {}
                        }
                    }
                    Ok(event) = ui_state_rx.recv() => {
                        if let crate::orchestrator::UiStateEvent::SetActiveAgentIdentity(identity) = event {
                            *active_agent_identity.lock().expect("active agent identity lock") = Some(identity);
                        }
                    }
                    else => break,
                }
            }
        }));
        self
    }

    /// Feeds the first prompt immediately, then injects agent-switch requests
    /// only while the worker is active. `Quit` is gated on the expected turn
    /// completion count (set by `with_bus`).
    fn make_event_spawner(&self) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let submitted = self.submitted.clone();
        let agent_switch_requests = self.agent_switch_requests.clone();
        let quit = Arc::clone(&self.quit);
        let drained_tx = Arc::clone(&self.drained_tx);
        move |event_tx| {
            let event_tx = event_tx.clone();
            let mut submitted = submitted.clone();
            let mut agent_switch_requests = agent_switch_requests.clone();
            tokio::spawn(async move {
                if let Some(prompt) = submitted.pop_front() {
                    let _ = event_tx
                        .send(OrchestratorEvent::PromptSubmitted { text: prompt })
                        .await;
                }
                while !quit.load(Ordering::SeqCst) {
                    if let Some(name) = agent_switch_requests.pop_front() {
                        let _ = event_tx
                            .send(OrchestratorEvent::UiRequest(UiRequest::SwitchAgent {
                                name,
                            }))
                            .await;
                    }
                    if agent_switch_requests.is_empty() {
                        let _ = drained_tx.send(true);
                    }
                    if quit.load(Ordering::SeqCst)
                        && event_tx.send(OrchestratorEvent::Quit).await.is_err()
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(2)).await;
                }
                let _ = event_tx.send(OrchestratorEvent::Quit).await;
            });
        }
    }
}

impl UserInputUi for ResponsiveAgentSwitchUi {
    fn event_sender(&self) -> &tokio::sync::mpsc::Sender<OrchestratorEvent> {
        &self.event_tx
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn agent_switch_sends_command_and_updates_ui_identity()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let bus = create_bus();
    let runtime = AgentSwitchRuntime {
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = AgentSwitchUi::new(&["research-agent"]).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let (runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e:?}"))?;

    assert!(value.is_nothing());
    assert_eq!(runtime.switched_agents, vec!["research-agent"]);
    assert_eq!(
        *ui.active_agent_identity
            .lock()
            .expect("active agent identity lock"),
        Some("research-agent".to_string())
    );
    assert_eq!(
        *ui.active_model_identity
            .lock()
            .expect("active model identity lock"),
        Some("openai/gpt-4o-mini".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn agent_switch_failure_warns_and_keeps_previous_agent()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let bus = create_bus();
    let runtime = AgentSwitchRuntime {
        switch_agent_result: Some(Err("agent not found".to_string())),
        bus: bus.clone(),
        ..Default::default()
    };
    let ui = AgentSwitchUi::new(&["nonexistent-agent"]).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e:?}"))?;

    assert!(value.is_nothing());
    assert!(
        ui.warnings
            .lock()
            .expect("warnings lock")
            .iter()
            .any(|w| w == "agent not found")
    );
    assert_eq!(
        *ui.active_agent_identity
            .lock()
            .expect("active agent identity lock"),
        None
    );
    Ok(())
}

#[tokio::test]
async fn agent_switch_while_worker_active_is_queued_for_next_turn()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningAgentRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let ui = ResponsiveAgentSwitchUi::new(&["first"], &["research-agent"], 1).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    // Unblock the turn once it has started and the fixture has injected every
    // queued agent switch.
    let mut turn_started_rx = runtime.turn_started_rx();
    let mut drained_rx = ui.drained_rx();
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        let _ = turn_started_rx.changed().await;
        let _ = drained_rx.changed().await;
        unblock.store(true, Ordering::SeqCst);
    });

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e:?}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        _runtime
            .switched_agents
            .lock()
            .expect("switched agents lock")
            .as_slice(),
        ["research-agent"]
    );
    assert_eq!(
        *ui.active_agent_identity
            .lock()
            .expect("active agent identity lock"),
        Some("research-agent".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn queued_agent_switch_last_write_wins()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let bus = create_bus();
    let block_first_turn = Arc::new(AtomicBool::new(false));
    let runtime = LongRunningAgentRuntime::new(Arc::clone(&block_first_turn)).with_bus(bus.clone());
    let ui =
        ResponsiveAgentSwitchUi::new(&["first"], &["agent-a", "agent-b"], 1).with_bus(bus.clone());
    let spawner = ui.make_event_spawner();

    // Unblock the turn once it has started and the fixture has injected every
    // queued agent switch.
    let mut turn_started_rx = runtime.turn_started_rx();
    let mut drained_rx = ui.drained_rx();
    let unblock = Arc::clone(&block_first_turn);
    tokio::spawn(async move {
        let _ = turn_started_rx.changed().await;
        let _ = drained_rx.changed().await;
        unblock.store(true, Ordering::SeqCst);
    });

    let (_runtime, result) = run_interactive_loop_impl(
        runtime,
        InteractiveLoopConfig::new(Span::test_data())
            .with_bus(bus)
            .with_spawn_render_loop(spawner),
    )
    .await;
    let value = result.map_err(|e| format!("interactive loop: {e:?}"))?;

    assert!(value.is_nothing());
    assert_eq!(
        _runtime
            .switched_agents
            .lock()
            .expect("switched agents lock")
            .as_slice(),
        ["agent-b"]
    );
    assert_eq!(
        *ui.active_agent_identity
            .lock()
            .expect("active agent identity lock"),
        Some("agent-b".to_string())
    );
    Ok(())
}
