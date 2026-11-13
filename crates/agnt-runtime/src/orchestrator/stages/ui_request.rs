use std::collections::{BTreeMap, VecDeque};

use crate::orchestrator::events::{UiRequest, UiRequestResponse, WorkerCommand};
use crate::orchestrator::stages::{OrchestrationContext, UiRequestHandler};
use crate::protocol::contracts::project_snapshots;
use agnt_kernel::bus::{FactRecord, LifecycleEvent, McpServerUsability, NarrationEvent};
use agnt_kernel::{NoticeKind, SessionLifecycle, SessionRecord};

/// Handles UI requests (model/agent/session switch, MCP toggle, picker refresh)
/// with a concurrency policy: blocking requests (model/agent/session) require an
/// idle worker and are queued when busy; the MCP toggle dispatches to the worker
/// immediately; a picker refresh is answered from the catalog reader, so it
/// needs no worker turn at all.
pub(crate) struct UiRequestStage {
    last_authoritative_visible_count: usize,
    /// The last visible tool count published per server.
    ///
    /// An abandoned toggle's substitute republishes these, so quitting while a
    /// toggle is unanswered leaves the footer on its last-known numbers instead
    /// of zeroing them.
    last_authoritative_counts: BTreeMap<String, usize>,
    /// The last visible tool names published per server, for the same reason.
    last_authoritative_names: BTreeMap<String, Vec<String>>,
    pending_blocking: Option<UiRequest>,
    pending_concurrent: Vec<UiRequest>,
    queued: VecDeque<UiRequest>,
}

impl UiRequestStage {
    pub fn new(initial_visible_count: usize) -> Self {
        Self {
            last_authoritative_visible_count: initial_visible_count,
            last_authoritative_counts: BTreeMap::new(),
            last_authoritative_names: BTreeMap::new(),
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
            let _ = ctx.narration.publish(NarrationEvent::SystemMessage {
                kind: NoticeKind::Warning,
                text: message,
            });
        }
    }

    async fn dispatch_concurrent(
        &mut self,
        server: String,
        enable: bool,
        ctx: &mut OrchestrationContext<'_>,
    ) {
        let request = UiRequest::ToggleMcp {
            server: server.clone(),
            enable,
        };
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
            let _ = ctx.facts.publish(FactRecord::McpServerState {
                server,
                state: McpServerUsability::Failed,
                error: Some("worker channel closed".to_string()),
            });
            let _ = ctx.facts.publish(FactRecord::McpVisibleToolTotal(
                self.last_authoritative_visible_count,
            ));
        }
    }

    /// Answer a picker request's own reply hand.
    ///
    /// The catalog is surface state: it goes back on the hand the surface
    /// supplied with the request, never on a bus channel. A surface that went
    /// away left no hand, and the catalog is dropped.
    fn answer_picker<T>(reply: &Option<crate::orchestrator::events::PickerReply<T>>, value: T) {
        if let Some(reply) = reply {
            let _ = reply.try_send(value);
        }
    }

    /// Tell the surface that a queued effect waits for the running turn.
    ///
    /// The queue is invisible otherwise: the switch looks accepted and nothing
    /// happens until the turn ends. One sentence per deferred request, published
    /// where the request lands.
    fn warn_deferred(&self, text: &str, ctx: &mut OrchestrationContext<'_>) {
        let _ = ctx.narration.publish(NarrationEvent::SystemMessage {
            kind: NoticeKind::Warning,
            text: text.to_string(),
        });
    }

    /// Name the gate that deferred a request.
    ///
    /// A queued request and a lost one look the same from the surface: one
    /// warning, then nothing. This line is the record of which gate was true at
    /// accept time, so a wedged flag is read from a log instead of guessed.
    fn log_deferred_gate(&self, request: &str, ctx: &OrchestrationContext<'_>) {
        log::warn!(
            "{request} queued: worker_active={} pending_blocking={}",
            *ctx.worker_active,
            self.pending_blocking.is_some()
        );
    }

    /// Remember the tool-count facts a toggle answer published.
    ///
    /// The stage is the one place that knows what the surface shows now, so an
    /// abandoned toggle's substitute reads its numbers from here.
    fn remember_tool_facts(
        &mut self,
        total: usize,
        server: &str,
        server_count: usize,
        names_by_server: &[(String, Vec<String>)],
    ) {
        self.last_authoritative_visible_count = total;
        self.last_authoritative_counts
            .insert(server.to_string(), server_count);
        for (srv, names) in names_by_server {
            self.last_authoritative_names
                .insert(srv.clone(), names.clone());
        }
    }

    /// The answer an abandoned request gets in place of the worker's.
    ///
    /// The text is the only thing a surface is told about a request that never
    /// answered, so it names both facts: what was abandoned, and that nothing
    /// arrived. A toggle's substitute carries the tool-count facts already
    /// published, so the footer keeps its last-known numbers; `None` for a
    /// request no lane carries (a picker refresh answers on the surface's own
    /// hand and never waits for the worker).
    fn abandonment(&self, request: &UiRequest) -> Option<UiRequestResponse> {
        let text = || {
            format!(
                "{} abandoned at quit: the response was not received",
                request.kind()
            )
        };
        match request {
            UiRequest::SwitchModel { .. } => Some(UiRequestResponse::ModelSwitch(Err(text()))),
            UiRequest::SwitchAgent { .. } => Some(UiRequestResponse::AgentSwitch(Err(text()))),
            UiRequest::SwitchSession { id } => Some(UiRequestResponse::SessionSwitch {
                id: id.clone(),
                result: Err(text()),
            }),
            UiRequest::ToggleMcp { server, .. } => Some(UiRequestResponse::McpToggle {
                server: server.clone(),
                result: Err(text()),
                total: self.last_authoritative_visible_count,
                server_count: self
                    .last_authoritative_counts
                    .get(server)
                    .copied()
                    .unwrap_or(0),
                names_by_server: self
                    .last_authoritative_names
                    .iter()
                    .map(|(srv, names)| (srv.clone(), names.clone()))
                    .collect(),
            }),
            UiRequest::RefreshSessionPicker { .. } | UiRequest::RefreshAgentPicker { .. } => None,
        }
    }
}

impl UiRequestHandler for UiRequestStage {
    async fn handle_incoming(&mut self, request: UiRequest, ctx: &mut OrchestrationContext<'_>) {
        match request {
            UiRequest::SwitchModel { spec } => {
                if *ctx.worker_active || self.pending_blocking.is_some() {
                    // The switch applies only once the worker is free, so the
                    // deferral is announced where the request lands, and the
                    // gate that deferred it is named in the log.
                    self.log_deferred_gate("model switch", ctx);
                    self.warn_deferred("model switch applies after the current turn finishes", ctx);
                    self.queued
                        .retain(|r| !matches!(r, UiRequest::SwitchModel { .. }));
                    self.queued.push_back(UiRequest::SwitchModel { spec });
                } else {
                    self.dispatch_blocking(UiRequest::SwitchModel { spec }, ctx)
                        .await;
                }
            }
            UiRequest::SwitchAgent { name } => {
                if *ctx.worker_active || self.pending_blocking.is_some() {
                    // As the model switch: queued behind the turn, said so, and
                    // the deferring gate named in the log.
                    self.log_deferred_gate("agent switch", ctx);
                    self.warn_deferred("agent switch applies after the current turn finishes", ctx);
                    self.queued
                        .retain(|r| !matches!(r, UiRequest::SwitchAgent { .. }));
                    self.queued.push_back(UiRequest::SwitchAgent { name });
                } else {
                    self.dispatch_blocking(UiRequest::SwitchAgent { name }, ctx)
                        .await;
                }
            }
            UiRequest::SwitchSession { id } => {
                if *ctx.worker_active {
                    let _ = ctx.narration.publish(NarrationEvent::SystemMessage {
                        kind: NoticeKind::Warning,
                        text: "Cannot switch session while worker is active".to_string(),
                    });
                } else if self.pending_blocking.is_none() {
                    self.dispatch_blocking(UiRequest::SwitchSession { id }, ctx)
                        .await;
                }
                // If pending_blocking is Some, ignore silently (no queue, no warning).
            }
            UiRequest::ToggleMcp { server, enable } => {
                self.dispatch_concurrent(server, enable, ctx).await;
            }
            UiRequest::RefreshSessionPicker { reply } => {
                let result = ctx.catalog.list_sessions().await;
                // A failed read still reaches the surface on its own hand; the
                // footer warning keeps the same sentence a switch failure
                // shows.
                if let Err(message) = &result {
                    let _ = ctx.narration.publish(NarrationEvent::SystemMessage {
                        kind: NoticeKind::Warning,
                        text: message.clone(),
                    });
                }
                Self::answer_picker(&reply, result);
            }
            UiRequest::RefreshAgentPicker { reply } => {
                Self::answer_picker(&reply, Ok(ctx.catalog.list_agents()));
            }
        }
    }

    async fn handle_blocking_response(
        &mut self,
        response: UiRequestResponse,
        ctx: &mut OrchestrationContext<'_>,
    ) {
        match response {
            UiRequestResponse::ModelSwitch(Ok((identity, max_tokens))) => {
                let _ = ctx.facts.publish(FactRecord::ActiveModelIdentity(identity));
                let _ = ctx
                    .facts
                    .publish(FactRecord::ContextWindowMaxTokens(max_tokens));
            }
            UiRequestResponse::ModelSwitch(Err(msg)) => {
                let _ = ctx.narration.publish(NarrationEvent::SystemMessage {
                    kind: NoticeKind::Warning,
                    text: msg,
                });
            }
            UiRequestResponse::AgentSwitch(Ok((
                agent_identity,
                model_identity,
                max_tokens,
                icon,
            ))) => {
                let _ = ctx
                    .facts
                    .publish(FactRecord::ActiveAgentIdentity(agent_identity));
                let _ = ctx.facts.publish(FactRecord::ActivePersonaIcon(icon));
                let _ = ctx
                    .facts
                    .publish(FactRecord::ActiveModelIdentity(model_identity));
                let _ = ctx
                    .facts
                    .publish(FactRecord::ContextWindowMaxTokens(max_tokens));
            }
            UiRequestResponse::AgentSwitch(Err(msg)) => {
                let _ = ctx.narration.publish(NarrationEvent::SystemMessage {
                    kind: NoticeKind::Warning,
                    text: msg,
                });
            }
            UiRequestResponse::SessionSwitch {
                id,
                result: Ok(snapshots),
            } => {
                // The switch marker and the replayed records ride the stream:
                // the surface folds one record log, so a rehydrated session and
                // a live one cannot disagree.
                let _ = ctx
                    .stream
                    .publish(SessionRecord::Session(SessionLifecycle::Switched {
                        from: None,
                        to: agnt_kernel::SessionId::new(id.clone()),
                    }));
                let (records, last_total_tokens) = project_snapshots(snapshots);
                for record in records {
                    let _ = ctx.stream.publish(record);
                }
                // The spine row is the switch's one announcement: the loop's
                // own arm moves the picker omission on it, so both switch paths
                // (this one and a peer attach) carry the same trigger.
                let _ = ctx
                    .lifecycle
                    .publish(LifecycleEvent::SessionSwitched { from: None, to: id });
                // The stored log carries no displayed total; the surface keeps
                // the one it had. Nothing is published for it.
                let _ = last_total_tokens;
            }
            UiRequestResponse::SessionSwitch {
                result: Err(msg), ..
            } => {
                let _ = ctx.narration.publish(NarrationEvent::SystemMessage {
                    kind: NoticeKind::Warning,
                    text: msg,
                });
            }
            _ => {}
        }
        self.pending_blocking = None;
    }

    async fn handle_concurrent_response(
        &mut self,
        response: UiRequestResponse,
        ctx: &mut OrchestrationContext<'_>,
    ) {
        let remove_mcp_server = match &response {
            UiRequestResponse::McpToggle { server, .. } => Some(server.clone()),
            _ => None,
        };
        match response {
            UiRequestResponse::McpToggle {
                server,
                result: Ok(state),
                total,
                server_count,
                names_by_server,
            } => {
                self.remember_tool_facts(total, &server, server_count, &names_by_server);
                let _ = ctx.facts.publish(FactRecord::McpVisibleToolCount {
                    server: server.clone(),
                    count: server_count,
                });
                let _ = ctx.facts.publish(FactRecord::McpVisibleToolTotal(total));
                for (srv, names) in names_by_server {
                    let _ = ctx
                        .facts
                        .publish(FactRecord::McpVisibleToolNames { server: srv, names });
                }
                let _ = ctx.facts.publish(FactRecord::McpServerState {
                    server,
                    state: state.into(),
                    error: None,
                });
            }
            UiRequestResponse::McpToggle {
                server,
                result: Err(err),
                total,
                server_count,
                names_by_server,
            } => {
                self.remember_tool_facts(total, &server, server_count, &names_by_server);
                let _ = ctx.facts.publish(FactRecord::McpVisibleToolCount {
                    server: server.clone(),
                    count: server_count,
                });
                let _ = ctx.facts.publish(FactRecord::McpVisibleToolTotal(total));
                for (srv, names) in names_by_server {
                    let _ = ctx
                        .facts
                        .publish(FactRecord::McpVisibleToolNames { server: srv, names });
                }
                let _ = ctx.facts.publish(FactRecord::McpServerState {
                    server,
                    state: McpServerUsability::Failed,
                    error: Some(err),
                });
            }
            _ => {}
        }
        // Remove the answered toggle's entry from pending_concurrent.
        if let Some(server) = &remove_mcp_server {
            self.pending_concurrent
                .retain(|r| !matches!(r, UiRequest::ToggleMcp { server: s, .. } if s == server));
        }
    }

    async fn drain_queued(&mut self, ctx: &mut OrchestrationContext<'_>) {
        if *ctx.worker_active || self.pending_blocking.is_some() {
            return;
        }
        if let Some(request) = self.queued.pop_front() {
            self.dispatch_blocking(request, ctx).await;
        }
    }

    /// Release every request that quit abandons, answering each one.
    ///
    /// A request the surface sent expects an answer: the modal it raised, the
    /// picker it opened, the toggle it flipped. Quit cannot wait for the worker's
    /// answer, so each abandoned request publishes its own substitute down the
    /// lane that answer would have taken. The loop folds it like any other
    /// response, so the surface releases instead of holding a wait nothing will
    /// ever fill.
    async fn abandon_unanswered(&mut self, ctx: &mut OrchestrationContext<'_>) {
        if let Some(request) = self.pending_blocking.take()
            && let Some(response) = self.abandonment(&request)
        {
            log::warn!(
                "abandoning the unanswered {} on quit: its response had not arrived when quit settled",
                request.kind()
            );
            let _ = ctx.blocking_response_tx.send(response).await;
        }
        for request in std::mem::take(&mut self.pending_concurrent) {
            let Some(response) = self.abandonment(&request) else {
                continue;
            };
            log::warn!(
                "abandoning the unanswered {} on quit: its response had not arrived when quit settled",
                request.kind()
            );
            let _ = ctx.concurrent_response_tx.send(response).await;
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
#[path = "../../../test/orchestrator/stages/ui_request.rs"]
mod ui_request_test;
