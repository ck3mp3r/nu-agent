use super::*;

pub(crate) struct FakeInteractiveUi {
    pub(crate) event_tx: mpsc::Sender<OrchestratorEvent>,
    pub(crate) submitted: Arc<Mutex<std::collections::VecDeque<String>>>,
    pub(crate) quit: Arc<AtomicBool>,
    pub(crate) min_bus_events: usize,
    pub(crate) call_order: Arc<Mutex<Vec<&'static str>>>,
    pub(crate) hydrated_messages: Arc<Mutex<Vec<UiMessageSnapshot>>>,
    pub(crate) clear_transcript_count: Arc<AtomicUsize>,
    pub(crate) push_startup_logo_count: Arc<AtomicUsize>,
    pub(crate) mcp_toggle_requests: Arc<Mutex<std::collections::VecDeque<McpToggleRequest>>>,
    pub(crate) mcp_details: Arc<Mutex<Vec<McpDetail>>>,
    pub(crate) mcp_visible_tool_count_updates: Arc<Mutex<Vec<(String, usize)>>>,
    pub(crate) warnings: Arc<Mutex<Vec<String>>>,
    pub(crate) expected_mcp_updates: usize,
    pub(crate) expected_compaction_events: usize,
    pub(crate) shared_actions: Arc<Mutex<Vec<SharedUiAction>>>,
    pub(crate) model_switch_requests: Arc<Mutex<std::collections::VecDeque<String>>>,
    pub(crate) active_model_identity: Arc<Mutex<Option<String>>>,
    pub(crate) agent_switch_requests: Arc<Mutex<std::collections::VecDeque<String>>>,
    pub(crate) session_switch_requests: Arc<Mutex<std::collections::VecDeque<String>>>,
    pub(crate) bus: Arc<Mutex<Option<crate::bus::Bus>>>,
    pub(crate) pending_external_prompts: Arc<Mutex<std::collections::VecDeque<String>>>,
    pub(crate) turn_active: Arc<AtomicBool>,
    pub(crate) expected_external_prompts: usize,
    pub(crate) external_prompts_seen: Arc<AtomicUsize>,
    pub(crate) _bus_task: Option<tokio::task::JoinHandle<()>>,
}

impl FakeInteractiveUi {
    pub(crate) fn with_prompts(prompts: &[&str]) -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
        Self {
            event_tx,
            submitted: Arc::new(Mutex::new(prompts.iter().map(|s| s.to_string()).collect())),
            quit: Arc::new(AtomicBool::new(false)),
            min_bus_events: 0,
            call_order: Arc::new(Mutex::new(Vec::new())),
            hydrated_messages: Arc::new(Mutex::new(Vec::new())),
            clear_transcript_count: Arc::new(AtomicUsize::new(0)),
            push_startup_logo_count: Arc::new(AtomicUsize::new(0)),
            mcp_toggle_requests: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            mcp_details: Arc::new(Mutex::new(Vec::<McpDetail>::new())),
            mcp_visible_tool_count_updates: Arc::new(Mutex::new(Vec::new())),
            warnings: Arc::new(Mutex::new(Vec::new())),
            expected_mcp_updates: 0,
            expected_compaction_events: 0,
            shared_actions: Arc::new(Mutex::new(Vec::new())),
            model_switch_requests: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            active_model_identity: Arc::new(Mutex::new(None)),
            agent_switch_requests: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            session_switch_requests: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            bus: Arc::new(Mutex::new(None)),
            pending_external_prompts: Arc::new(Mutex::new(std::collections::VecDeque::new())),
            turn_active: Arc::new(AtomicBool::new(false)),
            expected_external_prompts: 0,
            external_prompts_seen: Arc::new(AtomicUsize::new(0)),
            _bus_task: None,
        }
    }

    pub(crate) fn with_expected_mcp_updates(mut self, expected_mcp_updates: usize) -> Self {
        self.expected_mcp_updates = expected_mcp_updates;
        self
    }

    pub(crate) fn with_min_bus_events(mut self, min_bus_events: usize) -> Self {
        self.min_bus_events = min_bus_events;
        self
    }

    /// Require `count` A2A prompts to arrive on the UI bus before the mock's
    /// quit heuristic may fire. Without this the heuristic can quit on the
    /// startup metadata events, before the external prompt round-trips.
    pub(crate) fn with_expected_external_prompts(mut self, count: usize) -> Self {
        self.expected_external_prompts = count;
        self
    }

    pub(crate) fn with_expected_compaction_events(
        mut self,
        expected_compaction_events: usize,
    ) -> Self {
        self.expected_compaction_events = expected_compaction_events;
        self
    }

    pub(crate) fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        *self.bus.lock().expect("bus lock") = Some(bus.clone());
        let submitted = Arc::clone(&self.submitted);
        let mcp_toggle_requests = Arc::clone(&self.mcp_toggle_requests);
        let mcp_details = Arc::clone(&self.mcp_details);
        let mcp_visible_tool_count_updates = Arc::clone(&self.mcp_visible_tool_count_updates);
        let warnings = Arc::clone(&self.warnings);
        let shared_actions = Arc::clone(&self.shared_actions);
        let active_model_identity = Arc::clone(&self.active_model_identity);
        let call_order = Arc::clone(&self.call_order);
        let hydrated_messages = Arc::clone(&self.hydrated_messages);
        let quit = Arc::clone(&self.quit);
        let clear_transcript_count = Arc::clone(&self.clear_transcript_count);
        let push_startup_logo_count = Arc::clone(&self.push_startup_logo_count);
        let expected_mcp_updates = self.expected_mcp_updates;
        let expected_compaction_events = self.expected_compaction_events;
        let min_bus_events = self.min_bus_events;
        let bus_event_count = Arc::new(AtomicUsize::new(0));
        let compaction_rx_count = Arc::new(AtomicUsize::new(0));
        let pending_external_prompts = Arc::clone(&self.pending_external_prompts);
        let turn_active = Arc::clone(&self.turn_active);
        let expected_external_prompts = self.expected_external_prompts;
        let external_prompts_seen = Arc::clone(&self.external_prompts_seen);

        let mut ui_event_rx = bus.ui_event().subscribe();
        let mut ui_state_rx = bus.ui_state().subscribe();
        let mut compaction_rx = bus.compaction().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = ui_event_rx.recv() => {
                        let events = bus_event_count.fetch_add(1, Ordering::SeqCst) + 1;
                        match event {
                            UiEvent::Completed { .. } => {
                                // The turn ended. Clear the flag here rather than
                                // relying on the spawner, which consumes the same
                                // broadcast event and may run after this arm.
                                turn_active.store(false, Ordering::SeqCst);
                            }
                            UiEvent::Warning { message } | UiEvent::TurnError { message } => {
                                warnings.lock().expect("warnings lock").push(message);
                            }
                            _ => {}
                        }
                        let submitted_empty = submitted.lock().expect("submitted lock").is_empty();
                        let external_empty = pending_external_prompts
                            .lock()
                            .expect("pending external prompts lock")
                            .is_empty();
                        let mcp_toggle_empty = mcp_toggle_requests.lock().expect("mcp toggle lock").is_empty();
                        let mcp_details_len = mcp_details.lock().expect("mcp details lock").len();
                        let _shared_actions_len = shared_actions.lock().expect("shared actions lock").len();
                        if submitted_empty && external_empty && !turn_active.load(Ordering::SeqCst) && external_prompts_seen.load(Ordering::SeqCst) >= expected_external_prompts && mcp_toggle_empty && mcp_details_len >= expected_mcp_updates && events > min_bus_events {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    Ok(event) = ui_state_rx.recv() => {
                        let events = bus_event_count.fetch_add(1, Ordering::SeqCst) + 1;
                        match event {
                            UiStateEvent::SetMcpServerState { server, state, error, total } => {
                                mcp_details.lock().expect("mcp details lock").push((server, state, error, total));
                            }
                            UiStateEvent::SetMcpVisibleToolCount { server, count } => {
                                mcp_visible_tool_count_updates.lock().expect("mcp visible tool count lock").push((server, count));
                            }
                            UiStateEvent::ExecuteSharedUiAction(action) => {
                                shared_actions.lock().expect("shared actions lock").push(action);
                            }
                            UiStateEvent::SetActiveModelIdentity(identity) => {
                                *active_model_identity.lock().expect("active model identity lock") = Some(identity);
                            }
                            UiStateEvent::ClearTranscript => {
                                clear_transcript_count.fetch_add(1, Ordering::SeqCst);
                            }
                            UiStateEvent::PushStartupLogo => {
                                push_startup_logo_count.fetch_add(1, Ordering::SeqCst);
                            }
                            UiStateEvent::HydrateTranscript {
                                messages,
                                last_total_tokens,
                            } => {
                                call_order.lock().expect("call_order lock").push("hydrate");
                                hydrated_messages
                                    .lock()
                                    .expect("hydrated_messages lock")
                                    .extend(messages);
                                let _ = last_total_tokens;
                            }
                            UiStateEvent::EnqueueExternalPrompt { text } => {
                                // Mirror the TUI: an A2A prompt enters the prompt
                                // queue and is submitted back to the orchestrator.
                                // Mark the turn active here, before the spawner
                                // picks the prompt up, so the quit heuristic below
                                // cannot fire in the window between enqueue and
                                // dispatch.
                                turn_active.store(true, Ordering::SeqCst);
                                external_prompts_seen.fetch_add(1, Ordering::SeqCst);
                                pending_external_prompts
                                    .lock()
                                    .expect("pending external prompts lock")
                                    .push_back(text);
                            }
                            _ => {}
                        }
                        let submitted_empty = submitted.lock().expect("submitted lock").is_empty();
                        let external_empty = pending_external_prompts
                            .lock()
                            .expect("pending external prompts lock")
                            .is_empty();
                        let mcp_toggle_empty = mcp_toggle_requests.lock().expect("mcp toggle lock").is_empty();
                        let mcp_details_len = mcp_details.lock().expect("mcp details lock").len();
                        if submitted_empty && external_empty && !turn_active.load(Ordering::SeqCst) && external_prompts_seen.load(Ordering::SeqCst) >= expected_external_prompts && mcp_toggle_empty && mcp_details_len >= expected_mcp_updates && events > min_bus_events {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    Ok(_event) = compaction_rx.recv() => {
                        let events = bus_event_count.fetch_add(1, Ordering::SeqCst) + 1;
                        let compaction_count = compaction_rx_count.fetch_add(1, Ordering::SeqCst) + 1;
                        let submitted_empty = submitted.lock().expect("submitted lock").is_empty();
                        let mcp_toggle_empty = mcp_toggle_requests.lock().expect("mcp toggle lock").is_empty();
                        let mcp_details_len = mcp_details.lock().expect("mcp details lock").len();
                        if submitted_empty && !turn_active.load(Ordering::SeqCst) && external_prompts_seen.load(Ordering::SeqCst) >= expected_external_prompts && mcp_toggle_empty && mcp_details_len >= expected_mcp_updates && compaction_count >= expected_compaction_events && events > min_bus_events {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    else => break,
                }
            }
        }));
        self
    }

    /// Returns a closure that feeds this mock's queued inputs into the
    /// orchestrator's event channel, replacing the removed pull-based bridge.
    /// Wire it via `InteractiveLoopConfig::with_spawn_render_loop`.
    ///
    /// Feeds turn-producing prompts one at a time: a regular prompt is sent
    /// only after the previous turn completed, mirroring a user submitting the
    /// next prompt after the response. Slash commands are not gated.
    pub(crate) fn make_event_spawner(
        &self,
    ) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let submitted = Arc::clone(&self.submitted);
        let model_switch_requests = Arc::clone(&self.model_switch_requests);
        let agent_switch_requests = Arc::clone(&self.agent_switch_requests);
        let session_switch_requests = Arc::clone(&self.session_switch_requests);
        let mcp_toggle_requests = Arc::clone(&self.mcp_toggle_requests);
        let pending_external_prompts = Arc::clone(&self.pending_external_prompts);
        let turn_active = Arc::clone(&self.turn_active);
        let quit = Arc::clone(&self.quit);
        let bus = self
            .bus
            .lock()
            .expect("bus lock")
            .clone()
            .expect("FakeInteractiveUi must be constructed with with_bus");

        move |event_tx| {
            let event_tx = event_tx.clone();
            let mut ui_event_rx = bus.ui_event().subscribe();
            tokio::spawn(async move {
                // Send one turn-producing prompt (NotSlash) at a time, waiting
                // for TurnCompleted before sending the next. Slash commands
                // and switch requests are not gated.
                let mut turn_pending = false;
                loop {
                    if !turn_pending {
                        // An A2A prompt enqueued on the UI bus is submitted back
                        // to the orchestrator, mirroring the TUI prompt queue.
                        let external = pending_external_prompts
                            .lock()
                            .expect("pending external prompts lock")
                            .pop_front();
                        let text = external
                            .or_else(|| submitted.lock().expect("submitted lock").pop_front());
                        if let Some(text) = text {
                            let is_slash =
                                !matches!(parse_slash_command(&text), SlashParseResult::NotSlash);
                            if !is_slash {
                                turn_active.store(true, Ordering::SeqCst);
                            }
                            let _ = event_tx
                                .send(OrchestratorEvent::PromptSubmitted { text })
                                .await;
                            turn_pending = !is_slash;
                        }
                    }
                    loop {
                        let spec = model_switch_requests.lock().expect("ms lock").pop_front();
                        let Some(spec) = spec else {
                            break;
                        };
                        let _ = event_tx
                            .send(OrchestratorEvent::UiRequest(UiRequest::SwitchModel {
                                spec,
                            }))
                            .await;
                    }
                    loop {
                        let name = agent_switch_requests.lock().expect("as lock").pop_front();
                        let Some(name) = name else {
                            break;
                        };
                        let _ = event_tx
                            .send(OrchestratorEvent::UiRequest(UiRequest::SwitchAgent {
                                name,
                            }))
                            .await;
                    }
                    loop {
                        let id = session_switch_requests.lock().expect("ss lock").pop_front();
                        let Some(id) = id else {
                            break;
                        };
                        let _ = event_tx
                            .send(OrchestratorEvent::UiRequest(UiRequest::SwitchSession {
                                id,
                            }))
                            .await;
                    }
                    loop {
                        let req = mcp_toggle_requests.lock().expect("mt lock").pop_front();
                        let Some(req) = req else {
                            break;
                        };
                        let _ = event_tx
                            .send(OrchestratorEvent::UiRequest(UiRequest::ToggleMcp {
                                server: req.server_name,
                                enable: req.enable,
                            }))
                            .await;
                    }

                    if quit.load(Ordering::SeqCst)
                        && event_tx.send(OrchestratorEvent::Quit).await.is_err()
                    {
                        break;
                    }
                    if turn_pending {
                        while let Ok(event) = ui_event_rx.recv().await {
                            if matches!(event, UiEvent::Completed { .. }) {
                                turn_pending = false;
                                turn_active.store(false, Ordering::SeqCst);
                                break;
                            }
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            });
        }
    }
}

impl UserInputUi for FakeInteractiveUi {
    fn event_sender(&self) -> &mpsc::Sender<OrchestratorEvent> {
        &self.event_tx
    }
}
