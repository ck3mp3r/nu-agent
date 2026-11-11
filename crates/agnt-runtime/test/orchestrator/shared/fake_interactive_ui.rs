use super::*;

pub(crate) struct FakeInteractiveUi {
    pub(crate) event_tx: mpsc::Sender<OrchestratorEvent>,
    pub(crate) submitted: Arc<Mutex<std::collections::VecDeque<String>>>,
    pub(crate) quit: Arc<AtomicBool>,
    pub(crate) min_bus_events: usize,
    /// The order in which the surface observed the concerns it reads. `hydrate`
    /// is recorded when the stream's `SessionLifecycle::Started { hydrated: true }`
    /// arrives, which is the record the loop publishes first on a resumed
    /// session.
    pub(crate) call_order: Arc<Mutex<Vec<&'static str>>>,
    /// Every record the stream delivered, in order: the replay a resumed
    /// surface folds.
    pub(crate) stream_records: Arc<Mutex<Vec<SessionRecord>>>,
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
            stream_records: Arc::new(Mutex::new(Vec::new())),
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
        *self.bus.lock().unwrap_or_else(|e| e.into_inner()) = Some(bus.clone());
        let submitted = Arc::clone(&self.submitted);
        let mcp_toggle_requests = Arc::clone(&self.mcp_toggle_requests);
        let mcp_details = Arc::clone(&self.mcp_details);
        let mcp_visible_tool_count_updates = Arc::clone(&self.mcp_visible_tool_count_updates);
        let warnings = Arc::clone(&self.warnings);
        let shared_actions = Arc::clone(&self.shared_actions);
        let active_model_identity = Arc::clone(&self.active_model_identity);
        let call_order = Arc::clone(&self.call_order);
        let stream_records = Arc::clone(&self.stream_records);
        let quit = Arc::clone(&self.quit);
        let expected_mcp_updates = self.expected_mcp_updates;
        let expected_compaction_events = self.expected_compaction_events;
        let min_bus_events = self.min_bus_events;
        let bus_event_count = Arc::new(AtomicUsize::new(0));
        let compaction_rx_count = Arc::new(AtomicUsize::new(0));
        let turn_active = Arc::clone(&self.turn_active);
        let expected_external_prompts = self.expected_external_prompts;
        let external_prompts_seen = Arc::clone(&self.external_prompts_seen);

        let mut control_rx = bus.control().subscribe();
        let mut narration_rx = bus.narration().subscribe();
        let mut facts_rx = bus.facts().subscribe();
        let mut stream_rx = bus.stream().subscribe();
        let mut lifecycle_rx = bus.lifecycle().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            let mut mcp_total = 0usize;
            loop {
                tokio::select! {
                    Ok(event) = narration_rx.recv() => {
                        let events = bus_event_count.fetch_add(1, Ordering::SeqCst) + 1;
                        if let NarrationEvent::SystemMessage { text, .. } = event {
                            warnings.lock().unwrap_or_else(|e| e.into_inner()).push(text);
                        }
                        let submitted_empty = submitted.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
                        let mcp_toggle_empty = mcp_toggle_requests.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
                        let mcp_details_len = mcp_details.lock().unwrap_or_else(|e| e.into_inner()).len();
                        if submitted_empty && !turn_active.load(Ordering::SeqCst) && external_prompts_seen.load(Ordering::SeqCst) >= expected_external_prompts && mcp_toggle_empty && mcp_details_len >= expected_mcp_updates && events > min_bus_events {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    Ok(event) = lifecycle_rx.recv() => {
                        let events = bus_event_count.fetch_add(1, Ordering::SeqCst) + 1;
                        if let LifecycleEvent::TurnCompleted { .. } = event {
                            // The turn ended. Clear the flag here rather than
                            // relying on the spawner, which reads the same
                            // record and may run after this arm.
                            turn_active.store(false, Ordering::SeqCst);
                        }
                        let submitted_empty = submitted.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
                        let mcp_toggle_empty = mcp_toggle_requests.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
                        let mcp_details_len = mcp_details.lock().unwrap_or_else(|e| e.into_inner()).len();
                        if submitted_empty && !turn_active.load(Ordering::SeqCst) && external_prompts_seen.load(Ordering::SeqCst) >= expected_external_prompts && mcp_toggle_empty && mcp_details_len >= expected_mcp_updates && events > min_bus_events {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    Ok(fact) = facts_rx.recv() => {
                        let events = bus_event_count.fetch_add(1, Ordering::SeqCst) + 1;
                        match fact {
                            crate::bus::FactRecord::McpServerState { server, state, error } => {
                                let state = match state {
                                    crate::bus::McpServerUsability::Enabled => McpUsabilityState::Enabled,
                                    crate::bus::McpServerUsability::Disabled => McpUsabilityState::Disabled,
                                    crate::bus::McpServerUsability::Failed => McpUsabilityState::Failed,
                                };
                                mcp_details.lock().unwrap_or_else(|e| e.into_inner()).push((server, state, error, mcp_total));
                            }
                            crate::bus::FactRecord::McpVisibleToolTotal(total) => {
                                mcp_total = total;
                            }
                            crate::bus::FactRecord::McpVisibleToolCount { server, count } => {
                                mcp_visible_tool_count_updates.lock().unwrap_or_else(|e| e.into_inner()).push((server, count));
                            }
                            crate::bus::FactRecord::ActiveModelIdentity(identity) => {
                                *active_model_identity.lock().unwrap_or_else(|e| e.into_inner()) = Some(identity);
                            }
                            _ => {}
                        }
                        let submitted_empty = submitted.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
                        let mcp_toggle_empty = mcp_toggle_requests.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
                        let mcp_details_len = mcp_details.lock().unwrap_or_else(|e| e.into_inner()).len();
                        let _shared_actions_len = shared_actions.lock().unwrap_or_else(|e| e.into_inner()).len();
                        if submitted_empty && !turn_active.load(Ordering::SeqCst) && external_prompts_seen.load(Ordering::SeqCst) >= expected_external_prompts && mcp_toggle_empty && mcp_details_len >= expected_mcp_updates && events > min_bus_events {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    Ok(command) = control_rx.recv() => {
                        let events = bus_event_count.fetch_add(1, Ordering::SeqCst) + 1;
                        match command {
                            ControlCommand::SharedUiAction { action } => {
                                shared_actions.lock().unwrap_or_else(|e| e.into_inner()).push(action);
                            }
                            ControlCommand::EnqueueExternalPrompt { .. } => {
                                // The loop dispatches an external prompt itself:
                                // the command is an intake it performs on its own
                                // behalf, not a request for the surface to queue
                                // the prompt again. Count it so the quit
                                // heuristic waits for it, and mark the turn
                                // active before the dispatch lands.
                                turn_active.store(true, Ordering::SeqCst);
                                external_prompts_seen.fetch_add(1, Ordering::SeqCst);
                            }
                            _ => {}
                        }
                        let submitted_empty = submitted.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
                        let mcp_toggle_empty = mcp_toggle_requests.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
                        let mcp_details_len = mcp_details.lock().unwrap_or_else(|e| e.into_inner()).len();
                        if submitted_empty && !turn_active.load(Ordering::SeqCst) && external_prompts_seen.load(Ordering::SeqCst) >= expected_external_prompts && mcp_toggle_empty && mcp_details_len >= expected_mcp_updates && events > min_bus_events {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    Ok(record) = stream_rx.recv() => {
                        let events = bus_event_count.fetch_add(1, Ordering::SeqCst) + 1;
                        // A resumed session's replay opens with the hydrated
                        // marker; that record is the surface's proof the loop
                        // hydrated before it pumped anything.
                        if matches!(record, SessionRecord::Session(SessionLifecycle::Started { hydrated: true, .. })) {
                            call_order.lock().unwrap_or_else(|e| e.into_inner()).push("hydrate");
                        }
                        let compaction_count = if matches!(record, SessionRecord::Compaction(_)) {
                            compaction_rx_count.fetch_add(1, Ordering::SeqCst) + 1
                        } else {
                            compaction_rx_count.load(Ordering::SeqCst)
                        };
                        stream_records.lock().unwrap_or_else(|e| e.into_inner()).push(record);
                        let submitted_empty = submitted.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
                        let mcp_toggle_empty = mcp_toggle_requests.lock().unwrap_or_else(|e| e.into_inner()).is_empty();
                        let mcp_details_len = mcp_details.lock().unwrap_or_else(|e| e.into_inner()).len();
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
    /// Wire it via `LoopConfig::with_event_sink`.
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
        let turn_active = Arc::clone(&self.turn_active);
        let quit = Arc::clone(&self.quit);
        let Some(bus) = self.bus.lock().unwrap_or_else(|e| e.into_inner()).clone() else {
            panic!("FakeInteractiveUi must be constructed with with_bus");
        };

        move |event_tx| {
            let event_tx = event_tx.clone();
            let mut lifecycle_rx = bus.lifecycle().subscribe();
            tokio::spawn(async move {
                // Send one turn-producing prompt (NotSlash) at a time, waiting
                // for TurnCompleted before sending the next. Slash commands
                // and switch requests are not gated.
                let mut turn_pending = false;
                loop {
                    if !turn_pending {
                        let submission = submitted
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .pop_front();
                        if let Some(text) = submission {
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
                        let spec = model_switch_requests
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .pop_front();
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
                        let name = agent_switch_requests
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .pop_front();
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
                        let id = session_switch_requests
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .pop_front();
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
                        let req = mcp_toggle_requests
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .pop_front();
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
                        while let Ok(event) = lifecycle_rx.recv().await {
                            if matches!(event, LifecycleEvent::TurnCompleted { .. }) {
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
