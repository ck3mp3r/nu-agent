//! Shared fixtures for the router test modules.
//!
//! Holds the imports and the mock runtimes/UIs every topical router test file
//! needs. Sub-files pull them in with `use super::*;`.

pub(super) use nu_protocol::{Span, Value};
pub(super) use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
pub(super) use std::time::Duration;
pub(super) use tokio::sync::mpsc;

pub(super) use crate::bus::Bus;
pub(super) use crate::bus::LifecycleEvent;
pub(super) use crate::conversation::turn::TurnEnd;
pub(super) use crate::orchestrator::events::{
    OrchestratorEvent, UiRequest, UiRequestResponse, WorkerCommand,
};
pub(super) use crate::orchestrator::test_shared::{drive_loop, handed_back};
pub(super) use crate::orchestrator::turn_outcome::{TurnOutcome, TurnResult};
pub(super) use crate::orchestrator::{
    AgentSwitchHook, CommandRouter, DispatchSources, LoopConfig, NoAgentSwitchHook,
};
pub(super) use crate::protocol::McpManagement;
pub(super) use crate::protocol::contracts::{
    CoreRuntime, McpUsabilityState, UiMessageSnapshot, UserInputUi,
};
pub(super) use crate::protocol::model_switching::ModelSwitching;
pub(super) use crate::session::management::{SessionPersistence, SessionState};
pub(super) use agnt_kernel::{SessionLifecycle, SessionRecord};

// ---------------------------------------------------------------------------
// Dispatch sources
// ---------------------------------------------------------------------------

/// The channels a router test hands the dispatcher, cloned from the bus the
/// test subscribes to.
pub(super) fn dispatch_sources(bus: &crate::bus::Bus) -> DispatchSources {
    DispatchSources {
        control: bus.control().clone(),
        lifecycle: bus.lifecycle().clone(),
        stream: bus.stream().clone(),
    }
}

// ---------------------------------------------------------------------------
// Recording agent-switch hook
// ---------------------------------------------------------------------------

/// One recorded agent-switch invocation: identity, description, icon.
type AgentSwitchCall = (String, Option<String>, Option<String>);

/// Records every agent-switch invocation so a test can assert the arguments.
#[derive(Clone, Default)]
pub(super) struct RecordingAgentSwitchHook {
    pub(super) calls: Arc<Mutex<Vec<AgentSwitchCall>>>,
}

impl AgentSwitchHook for RecordingAgentSwitchHook {
    fn on_agent_switch(
        &self,
        agent_identity: String,
        agent_description: Option<String>,
        agent_icon: Option<String>,
    ) {
        self.calls.lock().unwrap_or_else(|e| e.into_inner()).push((
            agent_identity,
            agent_description,
            agent_icon,
        ));
    }
}

// ---------------------------------------------------------------------------
// Simple fake runtime with switch_agent support (idle worker)
// ---------------------------------------------------------------------------

pub(super) struct AgentSwitchRuntime {
    pub(super) switched_agents: Vec<String>,
    pub(super) switch_agent_result: Option<Result<String, String>>,
    pub(super) active_model: String,
    pub(super) bus: crate::bus::Bus,
    /// The turn end this double reports. `None` reports a completed turn.
    pub(super) turn_end: Option<TurnEnd>,
}

impl Default for AgentSwitchRuntime {
    fn default() -> Self {
        Self {
            switched_agents: Vec::new(),
            switch_agent_result: None,
            active_model: "openai/gpt-4o-mini".to_string(),
            bus: crate::bus::Bus::default(),
            turn_end: None,
        }
    }
}

impl CoreRuntime for AgentSwitchRuntime {
    async fn execute_turn(
        &mut self,
        _prompt: String,
        _context: Option<String>,
        span: Span,
        _task_id: Option<String>,
    ) -> TurnEnd {
        // The executor bracket owns the turn spine; the double publishes it
        // directly, carrying the attribution the dispatch handed it.
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnStarted {
            prompt: _prompt.clone(),
            task_id: _task_id.clone(),
        });
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnCompleted {
            task_id: _task_id,
            duration_ms: 0,
        });
        self.turn_end.clone().unwrap_or(TurnEnd::Completed {
            value: Value::nothing(span),
        })
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
// The picker reader defaults to an empty catalog: the fixture holds no persona
// scan root and no session store.
impl crate::catalog::PickerCatalogSource for AgentSwitchRuntime {}

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

        let mut lifecycle_rx = bus.lifecycle().subscribe();
        let mut narration_rx = bus.narration().subscribe();
        let mut facts_rx = bus.facts().subscribe();

        self._bus_task = Some(tokio::spawn(async move {
            loop {
                tokio::select! {
                    Ok(event) = lifecycle_rx.recv() => {
                        if let LifecycleEvent::TurnCompleted { .. } = event
                            && agent_switch_requests
                                .lock()
                                .unwrap_or_else(|e| e.into_inner())
                                .is_empty()
                        {
                            quit.store(true, Ordering::SeqCst);
                        }
                    }
                    Ok(event) = narration_rx.recv() => {
                        if let crate::bus::NarrationEvent::SystemMessage { text, .. } = event {
                            warnings.lock().unwrap_or_else(|e| e.into_inner()).push(text);
                        }
                    }
                    ok = facts_rx.recv() => {
                        if let Ok(fact) = ok {
                            match fact {
                                crate::bus::FactRecord::ActiveModelIdentity(identity) => {
                                    *active_model_identity.lock().unwrap_or_else(|e| e.into_inner()) =
                                        Some(identity);
                                }
                                crate::bus::FactRecord::ActiveAgentIdentity(identity) => {
                                    *active_agent_identity.lock().unwrap_or_else(|e| e.into_inner()) =
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
                            .unwrap_or_else(|e| e.into_inner());
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
    /// The session the double is attached to: what `current_session_id()`
    /// reports, so a resolved switch is visible to the router.
    pub(super) attached_session_id: Option<String>,
    /// When set, a load resolves the requested id to this store id and
    /// succeeds.
    pub(super) resolved_session_id: Option<String>,
    pub(super) active_model: String,
    pub(super) bus: crate::bus::Bus,
}

impl CoreRuntime for SwitchSessionRuntime {
    async fn execute_turn(
        &mut self,
        _prompt: String,
        _context: Option<String>,
        span: Span,
        _task_id: Option<String>,
    ) -> TurnEnd {
        // The executor bracket owns the turn spine; the double publishes it
        // directly, carrying the attribution the dispatch handed it.
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnStarted {
            prompt: _prompt.clone(),
            task_id: _task_id.clone(),
        });
        let _ = self.bus.lifecycle().publish(LifecycleEvent::TurnCompleted {
            task_id: _task_id,
            duration_ms: 0,
        });
        TurnEnd::Completed {
            value: Value::nothing(span),
        }
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
    fn current_session_id(&self) -> Option<&str> {
        self.attached_session_id.as_deref()
    }

    async fn load_session(&mut self, session_id: &str) -> Result<Vec<UiMessageSnapshot>, String> {
        self.loaded_session_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(session_id.to_string());
        let Some(resolved) = self.resolved_session_id.clone() else {
            return Err("Session loading not supported".to_string());
        };
        // A store resolves a partial id to a longer stored one; the runtime
        // attaches to that.
        self.attached_session_id = Some(resolved);
        Ok(Vec::new())
    }
}

crate::default_mcp!(SwitchSessionRuntime);
// The picker reader defaults to an empty catalog: the fixture holds no persona
// scan root and no session store.
impl crate::catalog::PickerCatalogSource for SwitchSessionRuntime {}

// ---------------------------------------------------------------------------
// Attach-context runtime — a session store that may refuse the derived key
// ---------------------------------------------------------------------------

/// The double every `AttachA2aContext` dispatch test drives.
///
/// It records the key it was asked to attach, replays the snapshots a stored
/// session would carry, and — when `attach_error` is set — reports the failure
/// a store refusing that key produces.
pub(super) struct AttachContextRuntime {
    pub(super) attached_session_id: Arc<Mutex<Option<String>>>,
    pub(super) cwd: std::path::PathBuf,
    pub(super) snapshots: Vec<UiMessageSnapshot>,
    pub(super) current_session_id: Option<String>,
    /// What the runtime was seeded with, if anything: the outer `Option` records
    /// that the call happened, the inner one carries its argument.
    pub(super) seeded_tokens: Option<Option<u64>>,
    /// The failure an attach reports, when the fixture models a store that
    /// refuses the derived key.
    pub(super) attach_error: Option<String>,
}

impl CoreRuntime for AttachContextRuntime {
    async fn execute_turn(
        &mut self,
        _prompt: String,
        _context: Option<String>,
        span: Span,
        _task_id: Option<String>,
    ) -> TurnEnd {
        TurnEnd::Completed {
            value: Value::nothing(span),
        }
    }
}

impl ModelSwitching for AttachContextRuntime {
    async fn switch_model(&mut self, _model_spec: &str) -> Result<(String, Option<u64>), String> {
        Err("model switching not supported".to_string())
    }

    async fn switch_agent(&mut self, _agent_name: &str) -> Result<String, String> {
        Err("agent switch not supported".to_string())
    }

    fn active_model_identity(&self) -> String {
        "openai/gpt-4o-mini".to_string()
    }

    fn max_context_tokens(&self) -> Option<u64> {
        None
    }
}

impl SessionState for AttachContextRuntime {
    fn seed_last_total_tokens(&mut self, tokens: Option<u64>) {
        self.seeded_tokens = Some(tokens);
    }
}

impl SessionPersistence for AttachContextRuntime {
    fn cwd(&self) -> &std::path::Path {
        &self.cwd
    }

    fn current_session_id(&self) -> Option<&str> {
        self.current_session_id.as_deref()
    }

    async fn attach_session(&mut self, session_id: &str) -> Result<Vec<UiMessageSnapshot>, String> {
        self.attached_session_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(session_id.to_string());
        if let Some(error) = self.attach_error.clone() {
            return Err(error);
        }
        self.current_session_id = Some(session_id.to_string());
        Ok(self.snapshots.clone())
    }
}

crate::default_mcp!(AttachContextRuntime);
