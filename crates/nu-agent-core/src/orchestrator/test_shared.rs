pub(crate) use nu_protocol::{LabeledError, Span, Value};
pub(crate) use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
pub(crate) use std::time::Duration;

pub(crate) use crate::bus::{CompactionEvent, ExternalEvent, TurnEvent, WarningEvent, create_bus};
pub(crate) use crate::orchestrator::{
    InteractiveLoopConfig, OrchestratorEvent, UiRequest, UiStateEvent, run_interactive_loop_impl,
    run_single_turn,
};
pub(crate) use crate::protocol::{
    contracts::{
        CoreRuntime, McpToggleRequest, McpUsabilityState, SharedUiAction, UiMessageSnapshot,
        UiMessageUsageSnapshot, UserInputUi,
    },
    event::{PermissionDecision, ToolDisplay, ToolDisplaySection},
    mcp_management::McpManagement,
    model_switching::ModelSwitching,
    session_management::{SessionPersistence, SessionState},
    slash::{SlashParseResult, parse_slash_command},
};
pub(crate) use tokio::sync::mpsc;

// Convenience: empty impl blocks that use the trait's default methods.
// Since the protocol traits now provide defaults, test runtimes that don't
// need custom behavior only need these empty blocks to satisfy trait bounds.
#[macro_export]
macro_rules! default_session {
    ($t:ty) => {
        impl SessionState for $t {}
        impl SessionPersistence for $t {}
    };
}
#[macro_export]
macro_rules! default_mcp {
    ($t:ty) => {
        impl McpManagement for $t {}
    };
}

pub(crate) type McpDetail = (String, McpUsabilityState, Option<String>, usize);

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

        let mut turn_rx = bus.turn().subscribe();
        let mut ui_state_rx = bus.ui_state().subscribe();
        let mut warning_rx = bus.warning().subscribe();
        let mut compaction_rx = bus.compaction().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = turn_rx.recv() => {
                        let events = bus_event_count.fetch_add(1, Ordering::SeqCst) + 1;
                        let _ = event;
                        let submitted_empty = submitted.lock().expect("submitted lock").is_empty();
                        let mcp_toggle_empty = mcp_toggle_requests.lock().expect("mcp toggle lock").is_empty();
                        let mcp_details_len = mcp_details.lock().expect("mcp details lock").len();
                        let _shared_actions_len = shared_actions.lock().expect("shared actions lock").len();
                        if submitted_empty && mcp_toggle_empty && mcp_details_len >= expected_mcp_updates && events > min_bus_events {
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
                            _ => {}
                        }
                        let submitted_empty = submitted.lock().expect("submitted lock").is_empty();
                        let mcp_toggle_empty = mcp_toggle_requests.lock().expect("mcp toggle lock").is_empty();
                        let mcp_details_len = mcp_details.lock().expect("mcp details lock").len();
                        if submitted_empty && mcp_toggle_empty && mcp_details_len >= expected_mcp_updates && events > min_bus_events {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    Ok(event) = warning_rx.recv() => {
                        let events = bus_event_count.fetch_add(1, Ordering::SeqCst) + 1;
                        match event {
                            WarningEvent::Message { message } | WarningEvent::TurnError { message } => {
                                warnings.lock().expect("warnings lock").push(message);
                            }
                        }
                        let submitted_empty = submitted.lock().expect("submitted lock").is_empty();
                        let mcp_toggle_empty = mcp_toggle_requests.lock().expect("mcp toggle lock").is_empty();
                        let mcp_details_len = mcp_details.lock().expect("mcp details lock").len();
                        if submitted_empty && mcp_toggle_empty && mcp_details_len >= expected_mcp_updates && events > min_bus_events {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    Ok(_event) = compaction_rx.recv() => {
                        let events = bus_event_count.fetch_add(1, Ordering::SeqCst) + 1;
                        let compaction_count = compaction_rx_count.fetch_add(1, Ordering::SeqCst) + 1;
                        let submitted_empty = submitted.lock().expect("submitted lock").is_empty();
                        let mcp_toggle_empty = mcp_toggle_requests.lock().expect("mcp toggle lock").is_empty();
                        let mcp_details_len = mcp_details.lock().expect("mcp details lock").len();
                        if submitted_empty && mcp_toggle_empty && mcp_details_len >= expected_mcp_updates && compaction_count >= expected_compaction_events && events > min_bus_events {
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
        let quit = Arc::clone(&self.quit);
        let bus = self
            .bus
            .lock()
            .expect("bus lock")
            .clone()
            .expect("FakeInteractiveUi must be constructed with with_bus");

        move |event_tx| {
            let event_tx = event_tx.clone();
            let mut turn_rx = bus.turn().subscribe();
            tokio::spawn(async move {
                // Send one turn-producing prompt (NotSlash) at a time, waiting
                // for TurnCompleted before sending the next. Slash commands
                // and switch requests are not gated.
                let mut turn_pending = false;
                loop {
                    if !turn_pending {
                        let text = submitted.lock().expect("submitted lock").pop_front();
                        if let Some(text) = text {
                            let is_slash =
                                !matches!(parse_slash_command(&text), SlashParseResult::NotSlash);
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
                        while let Ok(event) = turn_rx.recv().await {
                            if matches!(event, TurnEvent::Completed { .. }) {
                                turn_pending = false;
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

#[derive(Default)]
pub(crate) struct FakeRuntime {
    pub(crate) prompts: Vec<String>,
    pub(crate) run_compaction_calls: usize,
    pub(crate) run_compaction_sources: Vec<String>,
    pub(crate) run_compaction_fail: bool,
    pub(crate) switched_models: Vec<String>,
    pub(crate) switch_model_result: Option<Result<String, String>>,
    pub(crate) bus: crate::bus::Bus,
}

impl CoreRuntime for FakeRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        // The worker bridge no longer converts `UiEvent::Completed` to a
        // `TurnEvent::Completed` on the bus. Publish it directly, matching
        // production (see executor.rs).
        let _ = self
            .bus
            .turn()
            .send(TurnEvent::Completed { tool_calls: 0 })
            .await;
        Ok(Value::nothing(Span::test_data()))
    }
}

impl McpManagement for FakeRuntime {
    async fn set_mcp_server_enabled(
        &mut self,
        _name: &str,
        enabled: bool,
    ) -> Result<McpUsabilityState, String> {
        Ok(if enabled {
            McpUsabilityState::Enabled
        } else {
            McpUsabilityState::Disabled
        })
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

impl ModelSwitching for FakeRuntime {
    fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String> {
        self.switched_models.push(model_spec.to_string());
        if let Some(result) = self.switch_model_result.clone() {
            return result.map(|identity| (identity, None));
        }
        Ok((model_spec.to_string(), None))
    }

    fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported in this runtime".to_string())
    }

    fn active_model_identity(&self) -> String {
        "openai/gpt-4o-mini".to_string()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

impl SessionState for FakeRuntime {}

impl SessionPersistence for FakeRuntime {
    async fn run_compaction(&mut self, source: &str) -> Result<(), String> {
        self.run_compaction_calls = self.run_compaction_calls.saturating_add(1);
        self.run_compaction_sources.push(source.to_string());
        if self.run_compaction_fail {
            return Err("auto compaction failed".to_string());
        }
        // Publish lifecycle events directly to the bus so test-harness quit-gates
        // that subscribe to bus.compaction() fire (matching production, where the
        // compactor publishes CompactionEvent to the bus directly).
        let _ = self
            .bus
            .compaction()
            .send(CompactionEvent::Completed {
                source: source.to_string(),
                summary_preview: "summary".to_string(),
                summary_body: "summary".to_string(),
            })
            .await;
        Ok(())
    }
}

#[derive(Default)]
pub(crate) struct FakeValueRuntime {
    pub(crate) prompts: Vec<String>,
    pub(crate) bus: crate::bus::Bus,
}

impl CoreRuntime for FakeValueRuntime {
    async fn execute_turn(
        &mut self,
        _bus: &crate::bus::Bus,
        prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        self.prompts.push(prompt);
        // Publish TurnCompleted directly to the bus (worker bridge no longer
        // converts UiEvent::Completed), matching production.
        let _ = self
            .bus
            .turn()
            .send(TurnEvent::Completed { tool_calls: 0 })
            .await;
        Ok(Value::record(nu_protocol::Record::new(), span))
    }
}

impl ModelSwitching for FakeValueRuntime {
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

crate::default_session!(FakeValueRuntime);
crate::default_mcp!(FakeValueRuntime);

#[derive(Default)]
pub(crate) struct LongRunningRuntime {
    pub(crate) prompts: Arc<Mutex<Vec<String>>>,
    pub(crate) switched_models: Arc<Mutex<Vec<String>>>,
    pub(crate) action_log: Arc<Mutex<Vec<String>>>,
    pub(crate) active: Arc<AtomicBool>,
    pub(crate) block_first_turn: Arc<AtomicBool>,
    pub(crate) switch_model_result: Option<Result<String, String>>,
    pub(crate) active_identity: Arc<Mutex<String>>,
    pub(crate) bus: crate::bus::Bus,
}

impl LongRunningRuntime {
    pub(crate) fn new(block_first_turn: Arc<AtomicBool>) -> Self {
        Self {
            prompts: Arc::new(Mutex::new(Vec::new())),
            switched_models: Arc::new(Mutex::new(Vec::new())),
            action_log: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(AtomicBool::new(false)),
            block_first_turn,
            switch_model_result: None,
            active_identity: Arc::new(Mutex::new("openai/gpt-4o-mini".to_string())),
            bus: crate::bus::Bus::default(),
        }
    }

    pub(crate) fn with_switch_model_result(mut self, result: Result<String, String>) -> Self {
        self.switch_model_result = Some(result);
        self
    }

    pub(crate) fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        self.bus = bus;
        self
    }
}

impl CoreRuntime for LongRunningRuntime {
    async fn execute_turn(
        &mut self,
        bus: &crate::bus::Bus,
        prompt: String,
        _context: Option<String>,
        _span: Span,
    ) -> Result<Value, LabeledError> {
        self.active.store(true, Ordering::SeqCst);
        self.action_log
            .lock()
            .expect("action log lock")
            .push(format!("turn:{prompt}"));
        self.prompts
            .lock()
            .expect("prompts lock")
            .push(prompt.clone());

        if prompt == "first" {
            let mut cancel_rx = bus.cancel().subscribe();
            while !self.block_first_turn.load(Ordering::SeqCst) {
                if matches!(
                    cancel_rx.try_recv(),
                    Ok(crate::bus::CancelEvent::Requested)
                        | Err(crate::bus::TryRecvError::Lagged(_))
                ) {
                    self.active.store(false, Ordering::SeqCst);
                    return Err(LabeledError::new("LLM call cancelled"));
                }
                tokio::time::sleep(Duration::from_millis(2)).await;
            }
        }

        // Publish TurnCompleted directly to the bus (worker bridge no longer
        // converts UiEvent::Completed), matching production.
        let _ = self
            .bus
            .turn()
            .send(TurnEvent::Completed { tool_calls: 0 })
            .await;
        self.active.store(false, Ordering::SeqCst);
        Ok(Value::nothing(Span::test_data()))
    }
}

impl ModelSwitching for LongRunningRuntime {
    fn switch_model(&mut self, model_spec: &str) -> Result<(String, Option<u64>), String> {
        self.action_log
            .lock()
            .expect("action log lock")
            .push(format!("switch:{model_spec}"));
        self.switched_models
            .lock()
            .expect("switched models lock")
            .push(model_spec.to_string());

        if let Some(result) = self.switch_model_result.clone() {
            return result.map(|identity| (identity, None));
        }

        let mut identity = self.active_identity.lock().expect("identity lock");
        *identity = model_spec.to_string();
        Ok((model_spec.to_string(), None))
    }

    fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported in this runtime".to_string())
    }

    fn active_model_identity(&self) -> String {
        self.active_identity.lock().expect("identity lock").clone()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

crate::default_session!(LongRunningRuntime);
crate::default_mcp!(LongRunningRuntime);

pub(crate) struct ResponsiveInteractiveUi {
    pub(crate) event_tx: mpsc::Sender<OrchestratorEvent>,
    pub(crate) submitted: std::collections::VecDeque<String>,
    pub(crate) model_switch_requests: std::collections::VecDeque<String>,
    pub(crate) quit: Arc<AtomicBool>,
    pub(crate) active: Arc<AtomicBool>,
    pub(crate) completed_count: Arc<AtomicUsize>,
    pub(crate) expected_completions: usize,
    pub(crate) active_pump_count: Arc<AtomicUsize>,
    pub(crate) warnings: Arc<Mutex<Vec<String>>>,
    pub(crate) _bus_task: Option<tokio::task::JoinHandle<()>>,
}

impl ResponsiveInteractiveUi {
    pub(crate) fn new(
        initial_prompts: &[&str],
        injected_during_active: &[&str],
        injected_model_switch_during_active: &[&str],
        active: Arc<AtomicBool>,
        _block_first_turn: Arc<AtomicBool>,
        expected_completions: usize,
        active_pump_count: Arc<AtomicUsize>,
    ) -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
        let mut submitted: std::collections::VecDeque<String> =
            initial_prompts.iter().map(|s| s.to_string()).collect();
        submitted.extend(injected_during_active.iter().map(|s| s.to_string()));
        let model_switch_requests: std::collections::VecDeque<String> =
            injected_model_switch_during_active
                .iter()
                .map(|s| s.to_string())
                .collect();
        Self {
            event_tx,
            submitted,
            model_switch_requests,
            quit: Arc::new(AtomicBool::new(false)),
            active,
            completed_count: Arc::new(AtomicUsize::new(0)),
            expected_completions,
            active_pump_count,
            warnings: Arc::new(Mutex::new(Vec::new())),
            _bus_task: None,
        }
    }

    pub(crate) fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let quit = Arc::clone(&self.quit);
        let completed_count = Arc::clone(&self.completed_count);
        let active = Arc::clone(&self.active);
        let active_pump_count = Arc::clone(&self.active_pump_count);
        let warnings = Arc::clone(&self.warnings);
        let expected_completions = self.expected_completions;

        let mut turn_rx = bus.turn().subscribe();
        let mut warning_rx = bus.warning().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = turn_rx.recv() => {
                        if active.load(Ordering::SeqCst) {
                            active_pump_count.fetch_add(1, Ordering::SeqCst);
                        }
                        if let TurnEvent::Completed { .. } = event {
                            let count = completed_count.fetch_add(1, Ordering::SeqCst) + 1;
                            if count >= expected_completions {
                                quit.store(true, Ordering::SeqCst);
                            }
                        }
                    }
                    Ok(event) = warning_rx.recv() => {
                        match event {
                            WarningEvent::Message { message } | WarningEvent::TurnError { message } => {
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

    pub(crate) fn make_event_spawner(
        &self,
    ) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let submitted = self.submitted.clone();
        let model_switch_requests = self.model_switch_requests.clone();
        let active = Arc::clone(&self.active);
        let quit = Arc::clone(&self.quit);
        move |event_tx| {
            let event_tx = event_tx.clone();
            let mut submitted = submitted.clone();
            let mut model_switch_requests = model_switch_requests.clone();
            tokio::spawn(async move {
                // Send the first (initial) prompt immediately, then gate any
                // injected-during-active prompts and model switches on the
                // worker being active.
                if let Some(prompt) = submitted.pop_front() {
                    let _ = event_tx
                        .send(OrchestratorEvent::PromptSubmitted { text: prompt })
                        .await;
                }
                loop {
                    if active.load(Ordering::SeqCst) {
                        if let Some(prompt) = submitted.pop_front() {
                            let _ = event_tx
                                .send(OrchestratorEvent::PromptSubmitted { text: prompt })
                                .await;
                        }
                        if let Some(spec) = model_switch_requests.pop_front() {
                            let _ = event_tx
                                .send(OrchestratorEvent::UiRequest(UiRequest::SwitchModel {
                                    spec,
                                }))
                                .await;
                        }
                    }
                    if quit.load(Ordering::SeqCst)
                        && event_tx.send(OrchestratorEvent::Quit).await.is_err()
                    {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(1)).await;
                }
            });
        }
    }
}

impl UserInputUi for ResponsiveInteractiveUi {
    fn event_sender(&self) -> &mpsc::Sender<OrchestratorEvent> {
        &self.event_tx
    }
}

// Compile-time check: run_single_turn must accept anything that impls CoreRuntime
// This test will fail to compile until CoreRuntime exists and run_single_turn uses it
pub(crate) fn _assert_single_turn_accepts_core_runtime<R: CoreRuntime + Send>(_r: R) {
    // if this compiles, the bound is correct
}

// Compile-time check: the interactive loop must accept anything that impls the focused capability traits
pub(crate) fn _assert_interactive_loop_accepts_extended_runtime<
    R: CoreRuntime + McpManagement + ModelSwitching + SessionState + SessionPersistence + Send,
    U: UserInputUi,
>(
    _r: R,
    _u: U,
) {
    // if this compiles, the bound is correct
}

// ---------------------------------------------------------------------------
