//! Shared fixtures for the router test modules.
//!
//! Holds the imports and the mock runtimes/UIs every topical router test file
//! needs. Sub-files pull them in with `use super::*;`.

pub(super) use nu_protocol::{LabeledError, Span, Value};
pub(super) use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
pub(super) use std::time::Duration;
pub(super) use tokio::sync::mpsc;

pub(super) use crate::bus::create_bus;
pub(super) use crate::orchestrator::{
    CommandRouter, InteractiveLoopConfig, OnAgentSwitch, OrchestratorEvent, UiRequest,
    UiRequestResponse, WorkerCommand, run_interactive_loop_impl,
};
pub(super) use crate::protocol::{
    contracts::{CoreRuntime, McpUsabilityState, UiMessageSnapshot, UserInputUi},
    events::SystemEvent,
    mcp_management::McpManagement,
    model_switching::ModelSwitching,
    session_management::{SessionPersistence, SessionState},
};

// ---------------------------------------------------------------------------
// Simple fake runtime with switch_agent support (idle worker)
// ---------------------------------------------------------------------------

pub(super) struct AgentSwitchRuntime {
    pub(super) switched_agents: Vec<String>,
    pub(super) switch_agent_result: Option<Result<String, String>>,
    pub(super) active_model: String,
    pub(super) bus: crate::bus::Bus,
}

impl Default for AgentSwitchRuntime {
    fn default() -> Self {
        Self {
            switched_agents: Vec::new(),
            switch_agent_result: None,
            active_model: "openai/gpt-4o-mini".to_string(),
            bus: crate::bus::Bus::default(),
        }
    }
}

impl CoreRuntime for AgentSwitchRuntime {
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
            .send(SystemEvent::TurnCompleted {
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

impl McpManagement for AgentSwitchRuntime {
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

impl ModelSwitching for AgentSwitchRuntime {
    async fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    async fn switch_agent(&mut self, agent_name: &str) -> Result<String, String> {
        self.switched_agents.push(agent_name.to_string());
        if let Some(result) = self.switch_agent_result.clone() {
            return result;
        }
        Ok(agent_name.to_string())
    }

    fn active_model_identity(&self) -> String {
        self.active_model.clone()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

impl SessionState for AgentSwitchRuntime {
    fn clear_session(&mut self) {}
    fn new_session(&mut self) {}
    fn seed_last_total_tokens(&mut self, _tokens: Option<u64>) {}
}
impl SessionPersistence for AgentSwitchRuntime {}

// ---------------------------------------------------------------------------
// Simple fake interactive UI with agent switch request support
// ---------------------------------------------------------------------------

pub(super) struct AgentSwitchUi {
    event_tx: tokio::sync::mpsc::Sender<OrchestratorEvent>,
    agent_switch_requests: Arc<Mutex<std::collections::VecDeque<String>>>,
    quit: Arc<AtomicBool>,
    pub(super) warnings: Arc<Mutex<Vec<String>>>,
    pub(super) active_model_identity: Arc<Mutex<Option<String>>>,
    pub(super) active_agent_identity: Arc<Mutex<Option<String>>>,
    _bus_task: Option<tokio::task::JoinHandle<()>>,
}

impl AgentSwitchUi {
    pub(super) fn new(agent_switches: &[&str]) -> Self {
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<OrchestratorEvent>(256);
        Self {
            event_tx,
            agent_switch_requests: Arc::new(Mutex::new(
                agent_switches.iter().map(|s| s.to_string()).collect(),
            )),
            quit: Arc::new(AtomicBool::new(false)),
            warnings: Arc::new(Mutex::new(Vec::new())),
            active_model_identity: Arc::new(Mutex::new(None)),
            active_agent_identity: Arc::new(Mutex::new(None)),
            _bus_task: None,
        }
    }

    pub(super) fn with_bus(mut self, bus: crate::bus::Bus) -> Self {
        let quit = Arc::clone(&self.quit);
        let agent_switch_requests = Arc::clone(&self.agent_switch_requests);
        let warnings = Arc::clone(&self.warnings);
        let active_model_identity = Arc::clone(&self.active_model_identity);
        let active_agent_identity = Arc::clone(&self.active_agent_identity);

        let mut ui_event_rx = bus.ui_event().subscribe();
        let mut ui_state_rx = bus.ui_state().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = ui_event_rx.recv() => {
                        match event {
                            SystemEvent::TurnCompleted { .. } => {
                                if agent_switch_requests
                                    .lock()
                                    .expect("agent switch requests lock")
                                    .is_empty()
                                {
                                    quit.store(true, Ordering::SeqCst);
                                }
                            }
                            SystemEvent::SystemMessage { text, .. } => {
                                warnings.lock().expect("warnings lock").push(text);
                            }
                            _ => {}
                        }
                    }
                    ok = ui_state_rx.recv() => {
                        if let Ok(event) = ok {
                            match event {
                                crate::orchestrator::UiStateEvent::SetActiveModelIdentity(identity) => {
                                    *active_model_identity.lock().expect("active model identity lock") =
                                        Some(identity);
                                }
                                crate::orchestrator::UiStateEvent::SetActiveAgentIdentity(identity) => {
                                    *active_agent_identity.lock().expect("active agent identity lock") =
                                        Some(identity);
                                }
                                _ => {}
                            }
                        }
                    }
                    else => break,
                }
            }
        }));
        self
    }

    /// Feeds the queued agent-switch requests into the orchestrator's event
    /// channel and sends `Quit` once all have been dispatched.
    pub(super) fn make_event_spawner(
        &self,
    ) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let agent_switch_requests = Arc::clone(&self.agent_switch_requests);
        move |event_tx| {
            let event_tx = event_tx.clone();
            tokio::spawn(async move {
                loop {
                    let name = {
                        let mut guard = agent_switch_requests
                            .lock()
                            .expect("agent switch requests lock");
                        guard.pop_front()
                    };
                    let Some(name) = name else {
                        break;
                    };
                    let _ = event_tx
                        .send(OrchestratorEvent::UiRequest(UiRequest::SwitchAgent {
                            name,
                        }))
                        .await;
                }
                let _ = event_tx.send(OrchestratorEvent::Quit).await;
            });
        }
    }
}

impl UserInputUi for AgentSwitchUi {
    fn event_sender(&self) -> &tokio::sync::mpsc::Sender<OrchestratorEvent> {
        &self.event_tx
    }
}

// ---------------------------------------------------------------------------
// SwitchSession runtime — tracks load_session calls to verify async dispatch
// ---------------------------------------------------------------------------

pub(super) struct SwitchSessionRuntime {
    pub(super) loaded_session_id: Arc<Mutex<Option<String>>>,
    pub(super) active_model: String,
}

impl CoreRuntime for SwitchSessionRuntime {
    async fn execute_turn(
        &mut self,
        bus: &crate::bus::Bus,
        _prompt: String,
        _context: Option<String>,
        span: Span,
    ) -> Result<Value, LabeledError> {
        let _ = bus
            .ui_event()
            .send(SystemEvent::TurnCompleted {
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

impl ModelSwitching for SwitchSessionRuntime {
    async fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    async fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported".to_string())
    }

    fn active_model_identity(&self) -> String {
        self.active_model.clone()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

impl SessionState for SwitchSessionRuntime {
    fn clear_session(&mut self) {}
    fn new_session(&mut self) {}
    fn seed_last_total_tokens(&mut self, _tokens: Option<u64>) {}
}

impl SessionPersistence for SwitchSessionRuntime {
    async fn load_session(&mut self, session_id: &str) -> Result<Vec<UiMessageSnapshot>, String> {
        self.loaded_session_id
            .lock()
            .expect("loaded_session_id lock")
            .replace(session_id.to_string());
        Err("Session loading not supported".to_string())
    }
}

crate::default_mcp!(SwitchSessionRuntime);
