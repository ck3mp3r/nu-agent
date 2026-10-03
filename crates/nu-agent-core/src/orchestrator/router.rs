use crate::orchestrator::{
    OnAgentSwitch, UiRequest, UiRequestResponse, WorkerCommand, turn_outcome::TurnOutcome,
};
use crate::protocol::{
    contracts::CoreRuntime,
    mcp_management::McpManagement,
    model_switching::ModelSwitching,
    session_management::{SessionPersistence, SessionState},
};

use tokio::sync::mpsc;

/// Dispatches [`WorkerCommand`] variants to a runtime that implements all
/// focused capability traits.
///
/// This struct encapsulates the worker-thread dispatch logic that was previously
/// inline inside the interactive loop. Each match arm is identical to the
/// original orchestrator implementation — this is a pure extraction.
pub struct CommandRouter;

impl CommandRouter {
    /// Dispatch a single [`WorkerCommand`] to the runtime.
    ///
    /// Returns `true` if the worker loop should continue, `false` if it should
    /// shut down (i.e. `WorkerCommand::Shutdown`).
    ///
    /// `on_agent_switch` is an optional callback invoked after a successful
    /// agent switch, receiving the new agent's identity and description.
    pub async fn dispatch<R>(
        cmd: WorkerCommand,
        runtime: &mut R,
        result_tx: &mpsc::Sender<TurnOutcome>,
        on_agent_switch: Option<OnAgentSwitch>,
        bus: &crate::bus::Bus,
    ) -> bool
    where
        R: CoreRuntime + McpManagement + ModelSwitching + SessionState + SessionPersistence + Send,
    {
        match cmd {
            WorkerCommand::ExecuteTurn { prompt, span } => {
                log::debug!("Router: ExecuteTurn prompt_len={}", prompt.len());
                let result = runtime.execute_turn(bus, prompt, None, span).await;
                // Convert Result<Value, LabeledError> to TurnOutcome
                // Detect cancellation by message content:
                // - v2 path: "Turn cancelled: ..."
                let outcome = match &result {
                    Err(error) if error.msg.starts_with("Turn cancelled:") => {
                        TurnOutcome::Cancelled
                    }
                    Ok(value) => TurnOutcome::Success(value.clone()),
                    Err(error) => TurnOutcome::Error(error.clone()),
                };
                let _ = result_tx.send(outcome).await;
                true
            }
            WorkerCommand::RunCompaction { source } => {
                log::trace!("Router: RunCompaction source={source}");
                if let Err(e) = runtime.run_compaction(&source).await {
                    let _ = bus
                        .compaction()
                        .send(crate::bus::CompactionEvent::Failed { source, message: e })
                        .await;
                }
                true
            }
            WorkerCommand::HandleUiRequest {
                request,
                response_tx,
            } => {
                log::debug!("Router: HandleUiRequest");
                Self::dispatch_ui_request(request, runtime, &on_agent_switch, response_tx).await;
                true
            }
            WorkerCommand::AttachA2aContext { context_id, prompt } => {
                let prefix = crate::session::prefix::dir_prefix(runtime.cwd());
                let key = nu_agent_a2a::derive_session_key(&prefix, &context_id);
                log::info!("Router: AttachA2aContext context_id={context_id} key={key}");
                // Same-session attach: the transcript already shows this
                // session. Skip the clear/hydrate cycle so the incoming prompt
                // does not wipe the visible history.
                if runtime.current_session_id() == Some(key.as_str()) {
                    let _ = bus
                        .ui_state()
                        .send(crate::orchestrator::UiStateEvent::EnqueueExternalPrompt {
                            text: prompt,
                        })
                        .await;
                    return true;
                }
                match runtime.attach_session(&key).await {
                    Ok(snapshots) => {
                        let _ = bus
                            .session()
                            .send(crate::bus::SessionEvent::Switched {
                                from_session_id: None,
                                to_session_id: key,
                            })
                            .await;
                        // Clear first, then either show the startup logo (fresh
                        // session — mirrors the `/new` visual) or hydrate the
                        // stored transcript (resume). hydrate_from_messages does
                        // not clear, so the clear must precede it.
                        let _ = bus
                            .ui_state()
                            .send(crate::orchestrator::UiStateEvent::ClearTranscript)
                            .await;
                        if snapshots.is_empty() {
                            let _ = bus
                                .ui_state()
                                .send(crate::orchestrator::UiStateEvent::PushStartupLogo)
                                .await;
                        } else {
                            let _ = bus
                                .ui_state()
                                .send(crate::orchestrator::UiStateEvent::HydrateTranscript {
                                    messages: snapshots,
                                    last_total_tokens: None,
                                })
                                .await;
                        }
                    }
                    Err(e) => {
                        log::warn!("Failed to attach A2A context session '{key}': {e}");
                    }
                }
                // Enqueue the incoming message only after the transcript has
                // been replaced, so it never renders above the previous
                // session's history.
                let _ = bus
                    .ui_state()
                    .send(crate::orchestrator::UiStateEvent::EnqueueExternalPrompt { text: prompt })
                    .await;
                true
            }
            WorkerCommand::ClearSession => {
                log::info!("Router: ClearSession");
                runtime.clear_session();
                true
            }
            WorkerCommand::NewSession => {
                log::info!("Router: NewSession");
                runtime.new_session();
                true
            }
            WorkerCommand::Shutdown => {
                log::info!("Router: Shutdown");
                false
            }
        }
    }

    /// Dispatch a single [`UiRequest`] to the runtime and send the response
    /// through the provided channel.
    pub async fn dispatch_ui_request<R>(
        request: UiRequest,
        runtime: &mut R,
        on_agent_switch: &Option<OnAgentSwitch>,
        response_tx: mpsc::Sender<UiRequestResponse>,
    ) where
        R: CoreRuntime + McpManagement + ModelSwitching + SessionState + SessionPersistence + Send,
    {
        match request {
            UiRequest::SwitchModel { spec } => {
                log::debug!("Router: UiRequest SwitchModel spec={spec}");
                let result = runtime.switch_model(&spec);
                let _ = response_tx
                    .send(UiRequestResponse::ModelSwitch(result))
                    .await;
            }
            UiRequest::SwitchAgent { name } => {
                log::debug!("Router: UiRequest SwitchAgent name={name}");
                let result = runtime.switch_agent(&name);
                let response = result.map(|agent_identity| {
                    let model_identity = runtime.active_model_identity();
                    let max_tokens = runtime.max_context_tokens();
                    let icon = runtime.agent_icon().map(|s| s.to_string());
                    if let Some(cb) = on_agent_switch {
                        let description = runtime.agent_description().map(|s| s.to_string());
                        cb(agent_identity.clone(), description, icon.clone());
                    }
                    (agent_identity, model_identity, max_tokens, icon)
                });
                let _ = response_tx
                    .send(UiRequestResponse::AgentSwitch(response))
                    .await;
            }
            UiRequest::SwitchSession { id } => {
                log::debug!("Router: UiRequest SwitchSession id={id}");
                let result = runtime.load_session(&id).await;
                let _ = response_tx
                    .send(UiRequestResponse::SessionSwitch { id, result })
                    .await;
            }
            UiRequest::ToggleMcp { server, enable } => {
                log::debug!("Router: UiRequest ToggleMcp server={server} enable={enable}");
                let result = runtime.set_mcp_server_enabled(&server, enable).await;
                let total = runtime.llm_visible_mcp_tool_count();
                let server_count = runtime.llm_visible_mcp_tool_count_for_server(&server);
                let names_by_server = runtime.llm_visible_mcp_tool_names_by_server();
                let _ = response_tx
                    .send(UiRequestResponse::McpToggle {
                        server,
                        result,
                        total,
                        server_count,
                        names_by_server,
                    })
                    .await;
            }
            UiRequest::RefreshSessionPicker => {
                log::debug!("Router: UiRequest RefreshSessionPicker");
                let cwd = runtime.cwd();
                let result = runtime.list_sessions(cwd).await;
                let _ = response_tx
                    .send(UiRequestResponse::SessionRefresh(result))
                    .await;
            }
        }
    }
}

#[cfg(test)]
#[path = "router_test.rs"]
mod router_test;
