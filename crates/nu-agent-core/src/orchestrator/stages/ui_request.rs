use std::collections::VecDeque;

use crate::bus::{SessionEvent, WarningEvent};
use crate::orchestrator::stages::{OrchestrationContext, UiRequestHandler};
use crate::orchestrator::{UiRequest, UiRequestResponse, UiStateEvent, WorkerCommand};
use crate::protocol::contracts::McpUsabilityState;

/// Handles UI requests (model/agent/session switch, MCP toggle, session refresh)
/// with a concurrency policy: blocking requests (model/agent/session) require an
/// idle worker and are queued when busy; concurrent requests (MCP toggle, session
/// refresh) dispatch immediately.
pub(crate) struct UiRequestStage {
    last_authoritative_visible_count: usize,
    pending_blocking: Option<UiRequest>,
    pending_concurrent: Vec<UiRequest>,
    queued: VecDeque<UiRequest>,
}

impl UiRequestStage {
    pub fn new(initial_visible_count: usize) -> Self {
        Self {
            last_authoritative_visible_count: initial_visible_count,
            pending_blocking: None,
            pending_concurrent: Vec::new(),
            queued: VecDeque::new(),
        }
    }

    async fn dispatch_blocking(&mut self, request: UiRequest, ctx: &mut OrchestrationContext<'_>) {
        let response_tx = ctx.blocking_response_tx.clone();
        if ctx
            .worker_tx
            .send(WorkerCommand::HandleUiRequest {
                request: request.clone(),
                response_tx,
            })
            .await
            .is_ok()
        {
            self.pending_blocking = Some(request);
        } else {
            let message = match &request {
                UiRequest::SwitchModel { .. } => "Model switch worker channel closed".to_string(),
                UiRequest::SwitchAgent { .. } => "Agent switch worker channel closed".to_string(),
                UiRequest::SwitchSession { .. } => {
                    "Session switch worker channel closed".to_string()
                }
                _ => "Worker channel closed".to_string(),
            };
            let _ = ctx.bus.warning().send(WarningEvent::Message { message });
        }
    }

    async fn dispatch_concurrent(
        &mut self,
        request: UiRequest,
        ctx: &mut OrchestrationContext<'_>,
    ) {
        let response_tx = ctx.concurrent_response_tx.clone();
        if ctx
            .worker_tx
            .send(WorkerCommand::HandleUiRequest {
                request: request.clone(),
                response_tx,
            })
            .await
            .is_ok()
        {
            self.pending_concurrent.push(request);
        } else {
            match &request {
                UiRequest::ToggleMcp { server, .. } => {
                    let _ = ctx.bus.ui_state().send(UiStateEvent::SetMcpServerState {
                        server: server.clone(),
                        state: McpUsabilityState::Failed,
                        error: Some("worker channel closed".to_string()),
                        total: self.last_authoritative_visible_count,
                    });
                }
                _ => {
                    let _ = ctx.bus.warning().send(WarningEvent::Message {
                        message: "Worker channel closed".to_string(),
                    });
                }
            }
        }
    }
}

impl UiRequestHandler for UiRequestStage {
    async fn handle_incoming(&mut self, request: UiRequest, ctx: &mut OrchestrationContext<'_>) {
        match request {
            UiRequest::SwitchModel { spec } => {
                if *ctx.worker_active || self.pending_blocking.is_some() {
                    self.queued
                        .retain(|r| !matches!(r, UiRequest::SwitchModel { .. }));
                    self.queued
                        .push_back(UiRequest::SwitchModel { spec: spec.clone() });
                    let _ = ctx.bus.warning().send(WarningEvent::Message {
                        message: format!("Model switch queued for next turn: {spec}"),
                    });
                } else {
                    self.dispatch_blocking(UiRequest::SwitchModel { spec }, ctx)
                        .await;
                }
            }
            UiRequest::SwitchAgent { name } => {
                if *ctx.worker_active || self.pending_blocking.is_some() {
                    self.queued
                        .retain(|r| !matches!(r, UiRequest::SwitchAgent { .. }));
                    self.queued
                        .push_back(UiRequest::SwitchAgent { name: name.clone() });
                    let _ = ctx.bus.warning().send(WarningEvent::Message {
                        message: format!("Agent switch queued for next turn: {name}"),
                    });
                } else {
                    self.dispatch_blocking(UiRequest::SwitchAgent { name }, ctx)
                        .await;
                }
            }
            UiRequest::SwitchSession { id } => {
                if *ctx.worker_active {
                    let _ = ctx.bus.warning().send(WarningEvent::Message {
                        message: "Cannot switch session while worker is active".to_string(),
                    });
                } else if self.pending_blocking.is_none() {
                    self.dispatch_blocking(UiRequest::SwitchSession { id }, ctx)
                        .await;
                }
                // If pending_blocking is Some, ignore silently (no queue, no warning).
            }
            UiRequest::ToggleMcp { server, enable } => {
                self.dispatch_concurrent(UiRequest::ToggleMcp { server, enable }, ctx)
                    .await;
            }
            UiRequest::RefreshSessionPicker => {
                let already_in_flight = self
                    .pending_concurrent
                    .iter()
                    .any(|r| matches!(r, UiRequest::RefreshSessionPicker));
                if !already_in_flight {
                    self.dispatch_concurrent(UiRequest::RefreshSessionPicker, ctx)
                        .await;
                }
                // If already in-flight, skip silently.
            }
        }
    }

    fn handle_blocking_response(
        &mut self,
        response: UiRequestResponse,
        ctx: &mut OrchestrationContext,
    ) {
        match response {
            UiRequestResponse::ModelSwitch(Ok((identity, max_tokens))) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::SetActiveModelIdentity(identity.clone()));
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::SetContextWindowMaxTokens(max_tokens));
                let _ = ctx.bus.warning().send(WarningEvent::Message {
                    message: format!("Model switched: {identity}"),
                });
            }
            UiRequestResponse::ModelSwitch(Err(msg)) => {
                let _ = ctx
                    .bus
                    .warning()
                    .send(WarningEvent::Message { message: msg });
            }
            UiRequestResponse::AgentSwitch(Ok((
                agent_identity,
                model_identity,
                max_tokens,
                icon,
            ))) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::SetActiveAgentIdentity(agent_identity.clone()));
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::SetActivePersonaIcon(icon));
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::SetActiveModelIdentity(model_identity));
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::SetContextWindowMaxTokens(max_tokens));
                let _ = ctx.bus.warning().send(WarningEvent::Message {
                    message: format!("Agent switched to: {agent_identity}"),
                });
            }
            UiRequestResponse::AgentSwitch(Err(msg)) => {
                let _ = ctx
                    .bus
                    .warning()
                    .send(WarningEvent::Message { message: msg });
            }
            UiRequestResponse::SessionSwitch {
                id,
                result: Ok(snapshots),
            } => {
                let _ = ctx.bus.ui_state().send(UiStateEvent::ClearTranscript);
                let _ = ctx.bus.ui_state().send(UiStateEvent::HydrateTranscript {
                    messages: snapshots,
                    last_total_tokens: None,
                });
                let _ = ctx.bus.warning().send(WarningEvent::Message {
                    message: "Session switched".to_string(),
                });
                let _ = ctx.bus.session().send(SessionEvent::Switched {
                    from_session_id: None,
                    to_session_id: id,
                });
            }
            UiRequestResponse::SessionSwitch {
                result: Err(msg), ..
            } => {
                let _ = ctx
                    .bus
                    .warning()
                    .send(WarningEvent::Message { message: msg });
            }
            _ => {}
        }
        self.pending_blocking = None;
    }

    fn handle_concurrent_response(
        &mut self,
        response: UiRequestResponse,
        ctx: &mut OrchestrationContext,
    ) {
        let (remove_mcp_server, remove_session_refresh) = match &response {
            UiRequestResponse::McpToggle { server, .. } => (Some(server.clone()), false),
            UiRequestResponse::SessionRefresh(_) => (None, true),
            _ => (None, false),
        };
        match response {
            UiRequestResponse::McpToggle {
                server,
                result: Ok(state),
                total,
                server_count,
                names_by_server,
            } => {
                self.last_authoritative_visible_count = total;
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::SetMcpVisibleToolCount {
                        server: server.clone(),
                        count: server_count,
                    });
                for (srv, names) in names_by_server {
                    let _ = ctx
                        .bus
                        .ui_state()
                        .send(UiStateEvent::SetMcpVisibleToolNames { server: srv, names });
                }
                let _ = ctx.bus.ui_state().send(UiStateEvent::SetMcpServerState {
                    server,
                    state,
                    error: None,
                    total,
                });
            }
            UiRequestResponse::McpToggle {
                server,
                result: Err(err),
                total,
                server_count,
                names_by_server,
            } => {
                self.last_authoritative_visible_count = total;
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::SetMcpVisibleToolCount {
                        server: server.clone(),
                        count: server_count,
                    });
                for (srv, names) in names_by_server {
                    let _ = ctx
                        .bus
                        .ui_state()
                        .send(UiStateEvent::SetMcpVisibleToolNames { server: srv, names });
                }
                let _ = ctx.bus.ui_state().send(UiStateEvent::SetMcpServerState {
                    server,
                    state: McpUsabilityState::Failed,
                    error: Some(err),
                    total,
                });
            }
            UiRequestResponse::SessionRefresh(Ok(sessions)) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::SetSessionPickerOptions(sessions));
            }
            UiRequestResponse::SessionRefresh(Err(msg)) => {
                let _ = ctx
                    .bus
                    .warning()
                    .send(WarningEvent::Message { message: msg });
            }
            _ => {}
        }
        // Remove the matching entry from pending_concurrent.
        self.pending_concurrent.retain(|r| {
            if remove_session_refresh {
                return !matches!(r, UiRequest::RefreshSessionPicker);
            }
            if let Some(server) = &remove_mcp_server {
                return !matches!(r, UiRequest::ToggleMcp { server: s, .. } if s == server);
            }
            true
        });
    }

    async fn drain_queued(&mut self, ctx: &mut OrchestrationContext<'_>) {
        if *ctx.worker_active || self.pending_blocking.is_some() {
            return;
        }
        if let Some(request) = self.queued.pop_front() {
            self.dispatch_blocking(request, ctx).await;
        }
    }

    fn has_blocking_pending(&self) -> bool {
        self.pending_blocking.is_some()
    }

    fn has_pending(&self) -> bool {
        self.pending_blocking.is_some() || !self.pending_concurrent.is_empty()
    }
}

#[cfg(test)]
#[path = "ui_request_test.rs"]
mod ui_request_test;
