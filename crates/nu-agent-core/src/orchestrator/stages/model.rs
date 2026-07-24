use std::sync::mpsc as std_mpsc;

use crate::orchestrator::pending::PendingOps;
use crate::orchestrator::poll::{PollOutcome, poll_pending};
use crate::orchestrator::stages::{OrchestrationContext, StageOutcome};
use crate::orchestrator::{
    PendingAgentSwitch, PendingMcpToggle, PendingModelSwitch, PendingSessionSwitch, WorkerCommand,
};
use crate::protocol::contracts::{
    DisplayStateUi, LifecycleUi, McpToggleRequest, McpUsabilityState, ProgressUi, SharedUiAction,
    TranscriptUi, UserInputUi,
};
use crate::protocol::event::UiEvent;

/// Handles all model switching, agent switching, and MCP toggle operations.
///
/// Covers:
/// - Dispatching MCP toggle requests from the UI to the worker
/// - Polling pending model switch / agent switch / MCP toggle results
/// - Launching model / agent picker dialogs
/// - Queuing switches that arrive while the worker is busy
pub(crate) struct ModelSwitchStage {
    /// Last authoritative LLM-visible tool count; used as fallback for error messages.
    last_authoritative_visible_count: usize,
    pending_model_switch: Option<PendingModelSwitch>,
    pending_agent_switch: Option<PendingAgentSwitch>,
    pending_session_switch: Option<PendingSessionSwitch>,
    pending_mcp_toggles: Vec<PendingMcpToggle>,
    pending_ops: PendingOps,
}

impl ModelSwitchStage {
    pub fn new(initial_visible_count: usize) -> Self {
        Self {
            last_authoritative_visible_count: initial_visible_count,
            pending_model_switch: None,
            pending_agent_switch: None,
            pending_session_switch: None,
            pending_mcp_toggles: Vec::new(),
            pending_ops: PendingOps::new(),
        }
    }

    pub async fn poll<U>(&mut self, ctx: &mut OrchestrationContext<'_, U>) -> StageOutcome
    where
        U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
    {
        let mut handled = false;

        // --- Dispatch new MCP toggle requests from UI ---
        while let Some(McpToggleRequest {
            server_name,
            enable,
        }) = ctx.ui.take_next_mcp_toggle_request()
        {
            log::trace!("Dequeued MCP toggle: server={server_name} enable={enable}");
            let (response_tx, response_rx) = std_mpsc::channel();
            let send_result = ctx
                .worker_tx
                .send(WorkerCommand::ToggleMcp {
                    server_name: server_name.clone(),
                    enable,
                    response_tx,
                })
                .await;

            if send_result.is_err() {
                log::warn!("ToggleMcp send failed (channel closed): server={server_name}");
                ctx.ui.set_mcp_server_state_with_details(
                    &server_name,
                    McpUsabilityState::Failed,
                    Some("worker channel closed".to_string()),
                    self.last_authoritative_visible_count,
                );
                handled = true;
                continue;
            }

            log::trace!("ToggleMcp dispatched to worker: server={server_name} enable={enable}");
            self.pending_mcp_toggles.push((server_name, response_rx));
            handled = true;
        }

        // --- Poll pending model switch result ---
        if let Some(response_rx) = self.pending_model_switch.take() {
            match poll_pending(response_rx) {
                PollOutcome::Ready(Ok((active_identity, max_tokens))) => {
                    log::debug!("Model switch succeeded: {active_identity}");
                    ctx.ui.set_active_model_identity(active_identity.as_str());
                    ctx.ui.set_context_window_max_tokens(max_tokens);
                    ctx.ui.emit(&UiEvent::Warning {
                        message: format!("Model switched: {active_identity}"),
                    });
                    handled = true;
                }
                PollOutcome::Ready(Err(message)) => {
                    log::warn!("Model switch failed: {message}");
                    ctx.ui.emit(&UiEvent::Warning { message });
                    handled = true;
                }
                PollOutcome::Pending(rx) => self.pending_model_switch = Some(rx),
                PollOutcome::Disconnected => {
                    ctx.ui.emit(&UiEvent::Warning {
                        message: "Model switch worker disconnected".to_string(),
                    });
                    handled = true;
                }
            }
        }

        // --- Poll pending agent switch result ---
        if let Some(response_rx) = self.pending_agent_switch.take() {
            match poll_pending(response_rx) {
                PollOutcome::Ready(Ok((agent_identity, model_identity, max_tokens))) => {
                    log::debug!("Agent switch succeeded: {agent_identity}");
                    ctx.ui.set_active_agent_identity(&agent_identity);
                    ctx.ui.set_active_model_identity(&model_identity);
                    ctx.ui.set_context_window_max_tokens(max_tokens);
                    ctx.ui.emit(&UiEvent::Warning {
                        message: format!("Agent switched to: {agent_identity}"),
                    });
                    handled = true;
                }
                PollOutcome::Ready(Err(message)) => {
                    log::warn!("Agent switch failed: {message}");
                    ctx.ui.emit(&UiEvent::Warning { message });
                    handled = true;
                }
                PollOutcome::Pending(rx) => self.pending_agent_switch = Some(rx),
                PollOutcome::Disconnected => {
                    ctx.ui.emit(&UiEvent::Warning {
                        message: "Agent switch worker channel closed".to_string(),
                    });
                    handled = true;
                }
            }
        }

        // --- Poll pending session switch result ---
        if let Some(response_rx) = self.pending_session_switch.take() {
            match poll_pending(response_rx) {
                PollOutcome::Ready(Ok(snapshots)) => {
                    log::debug!("Session switch succeeded: {} messages", snapshots.len());
                    ctx.ui.clear_transcript();
                    ctx.ui.hydrate_transcript_from_messages(snapshots, None);
                    ctx.ui.emit(&UiEvent::Warning {
                        message: "Session switched".to_string(),
                    });
                    handled = true;
                }
                PollOutcome::Ready(Err(message)) => {
                    log::warn!("Session switch failed: {message}");
                    ctx.ui.emit(&UiEvent::Warning { message });
                    handled = true;
                }
                PollOutcome::Pending(rx) => self.pending_session_switch = Some(rx),
                PollOutcome::Disconnected => {
                    ctx.ui.emit(&UiEvent::Warning {
                        message: "Session switch worker disconnected".to_string(),
                    });
                    handled = true;
                }
            }
        }

        // --- Poll pending MCP toggle results ---
        let mut retained = Vec::new();
        for (server_name, response_rx) in self.pending_mcp_toggles.drain(..) {
            match response_rx.try_recv() {
                Ok((
                    Ok(state),
                    visible_count,
                    visible_count_for_server,
                    visible_names_by_server,
                )) => {
                    log::debug!(
                        "MCP toggle succeeded: server={server_name} state={state:?} visible_count={visible_count}"
                    );
                    self.last_authoritative_visible_count = visible_count;
                    ctx.ui.set_mcp_visible_tool_count_by_server_name(
                        &server_name,
                        visible_count_for_server,
                    );
                    for (server, names) in visible_names_by_server {
                        ctx.ui
                            .set_mcp_visible_tool_names_by_server_name(&server, names);
                    }
                    ctx.ui.set_mcp_server_state_with_details(
                        &server_name,
                        state,
                        None,
                        visible_count,
                    );
                    handled = true;
                }
                Ok((
                    Err(err),
                    visible_count,
                    visible_count_for_server,
                    visible_names_by_server,
                )) => {
                    log::warn!("MCP toggle failed: server={server_name} error={err}");
                    self.last_authoritative_visible_count = visible_count;
                    ctx.ui.set_mcp_visible_tool_count_by_server_name(
                        &server_name,
                        visible_count_for_server,
                    );
                    for (server, names) in visible_names_by_server {
                        ctx.ui
                            .set_mcp_visible_tool_names_by_server_name(&server, names);
                    }
                    ctx.ui.set_mcp_server_state_with_details(
                        &server_name,
                        McpUsabilityState::Failed,
                        Some(err),
                        visible_count,
                    );
                    handled = true;
                }
                Err(std_mpsc::TryRecvError::Empty) => retained.push((server_name, response_rx)),
                Err(std_mpsc::TryRecvError::Disconnected) => {
                    log::warn!("MCP toggle worker disconnected: server={server_name}");
                    ctx.ui.set_mcp_server_state_with_details(
                        &server_name,
                        McpUsabilityState::Failed,
                        Some("toggle worker disconnected".to_string()),
                        self.last_authoritative_visible_count,
                    );
                    handled = true;
                }
            }
        }
        self.pending_mcp_toggles = retained;

        // --- Model/agent/session picker launch requests ---
        while ctx.ui.take_next_model_picker_launch_request() {
            let _ = ctx.ui.execute_shared_ui_action(SharedUiAction::Models);
            handled = true;
        }

        while ctx.ui.take_next_agent_picker_launch_request() {
            let _ = ctx.ui.execute_shared_ui_action(SharedUiAction::Agents);
            handled = true;
        }

        while ctx.ui.take_next_session_picker_launch_request() {
            let _ = ctx.ui.execute_shared_ui_action(SharedUiAction::Sessions);
            handled = true;
        }

        // --- Model switch requests from UI ---
        while let Some(model_spec) = ctx.ui.take_next_model_switch_request() {
            handled = true;
            if *ctx.worker_active {
                self.pending_ops.queue_model_switch(model_spec.clone());
                ctx.ui.emit(&UiEvent::Warning {
                    message: format!("Model switch queued for next turn: {model_spec}"),
                });
            } else if self.pending_model_switch.is_none() {
                let (response_tx, response_rx) = std_mpsc::channel();
                if ctx
                    .worker_tx
                    .send(WorkerCommand::SwitchModel {
                        model_spec,
                        response_tx,
                    })
                    .await
                    .is_ok()
                {
                    self.pending_model_switch = Some(response_rx);
                } else {
                    ctx.ui.emit(&UiEvent::Warning {
                        message: "Model switch worker channel closed".to_string(),
                    });
                }
            } else {
                self.pending_ops.queue_model_switch(model_spec);
            }
        }

        // --- Agent switch requests from UI ---
        while let Some(agent_name) = ctx.ui.take_next_agent_switch_request() {
            handled = true;
            if *ctx.worker_active {
                self.pending_ops.queue_agent_switch(agent_name.clone());
                ctx.ui.emit(&UiEvent::Warning {
                    message: format!("Agent switch queued for next turn: {agent_name}"),
                });
            } else if self.pending_agent_switch.is_none() {
                let (response_tx, response_rx) = std_mpsc::channel();
                if ctx
                    .worker_tx
                    .send(WorkerCommand::SwitchAgent {
                        agent_name,
                        response_tx,
                    })
                    .await
                    .is_ok()
                {
                    self.pending_agent_switch = Some(response_rx);
                } else {
                    ctx.ui.emit(&UiEvent::Warning {
                        message: "Agent switch worker channel closed".to_string(),
                    });
                }
            } else {
                self.pending_ops.queue_agent_switch(agent_name);
            }
        }

        // --- Session switch requests from UI ---
        while let Some(session_id) = ctx.ui.take_next_session_switch_request() {
            handled = true;
            if *ctx.worker_active {
                ctx.ui.emit(&UiEvent::Warning {
                    message: "Cannot switch session while worker is active".to_string(),
                });
            } else if self.pending_session_switch.is_none() {
                let (response_tx, response_rx) = std_mpsc::channel();
                if ctx
                    .worker_tx
                    .send(WorkerCommand::SwitchSession {
                        session_id,
                        response_tx,
                    })
                    .await
                    .is_ok()
                {
                    self.pending_session_switch = Some(response_rx);
                } else {
                    ctx.ui.emit(&UiEvent::Warning {
                        message: "Session switch worker channel closed".to_string(),
                    });
                }
            }
        }

        // --- Drain queued model switch when worker is idle ---
        if !*ctx.worker_active
            && self.pending_model_switch.is_none()
            && let Some(model_spec) = self.pending_ops.take_queued_model_switch()
        {
            let (response_tx, response_rx) = std_mpsc::channel();
            if ctx
                .worker_tx
                .send(WorkerCommand::SwitchModel {
                    model_spec,
                    response_tx,
                })
                .await
                .is_ok()
            {
                self.pending_model_switch = Some(response_rx);
            } else {
                ctx.ui.emit(&UiEvent::Warning {
                    message: "Model switch worker channel closed".to_string(),
                });
            }
            handled = true;
        }

        // --- Drain queued agent switch when worker is idle ---
        if !*ctx.worker_active
            && self.pending_agent_switch.is_none()
            && let Some(agent_name) = self.pending_ops.take_queued_agent_switch()
        {
            let (response_tx, response_rx) = std_mpsc::channel();
            if ctx
                .worker_tx
                .send(WorkerCommand::SwitchAgent {
                    agent_name,
                    response_tx,
                })
                .await
                .is_ok()
            {
                self.pending_agent_switch = Some(response_rx);
            } else {
                ctx.ui.emit(&UiEvent::Warning {
                    message: "Agent switch worker channel closed".to_string(),
                });
            }
            handled = true;
        }

        if handled {
            StageOutcome::Handled
        } else {
            StageOutcome::Idle
        }
    }

    /// Returns `true` if any pending async operations are in flight (model switch,
    /// agent switch, or MCP toggles). Used by the main loop's quit check.
    pub fn has_pending(&self) -> bool {
        self.pending_model_switch.is_some()
            || self.pending_agent_switch.is_some()
            || !self.pending_mcp_toggles.is_empty()
    }

    /// Returns `true` if a model switch is currently in flight.
    /// Used by `OrchestratorStages::poll_all` to gate prompt dispatch.
    pub fn has_pending_model_switch(&self) -> bool {
        self.pending_model_switch.is_some()
    }
}
