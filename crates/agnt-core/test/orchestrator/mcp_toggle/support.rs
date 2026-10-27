use super::*;

// ── McpToggleRuntime ────────────────────────────────────────────────────

pub(super) struct McpToggleRuntime {
    pub(super) toggles: Vec<(String, bool)>,
    pub(super) next_state: McpUsabilityState,
    pub(super) visible_count: usize,
    pub(super) visible_count_by_server: usize,
    pub(super) bus: crate::bus::Bus,
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
    async fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    async fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
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

pub(super) struct FailingMcpToggleRuntime {
    pub(super) toggles: Vec<(String, bool)>,
    pub(super) visible_count: usize,
    pub(super) bus: crate::bus::Bus,
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
    async fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    async fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
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

pub(super) struct SequencedMcpToggleRuntime {
    pub(super) toggles: Vec<(String, bool)>,
    pub(super) states: std::collections::VecDeque<McpUsabilityState>,
    pub(super) visible_counts: std::collections::VecDeque<usize>,
    pub(super) visible_counts_by_server: std::collections::VecDeque<usize>,
    pub(super) current_visible_count: usize,
    pub(super) current_visible_count_by_server: usize,
    pub(super) bus: crate::bus::Bus,
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
    async fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    async fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
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

pub(super) struct StagedToggleUi {
    pub(super) event_tx: mpsc::Sender<OrchestratorEvent>,
    pub(super) quit: Arc<AtomicBool>,
    pub(super) first_response_processed: Arc<AtomicBool>,
    pub(super) second_response_processed: Arc<AtomicBool>,
    pub(super) _ui_state_task: Option<tokio::task::JoinHandle<()>>,
}

impl StagedToggleUi {
    pub(super) fn new() -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
        Self {
            event_tx,
            quit: Arc::new(AtomicBool::new(false)),
            first_response_processed: Arc::new(AtomicBool::new(false)),
            second_response_processed: Arc::new(AtomicBool::new(false)),
            _ui_state_task: None,
        }
    }

    pub(super) fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
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
    pub(super) fn make_event_spawner(
        &self,
    ) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
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
