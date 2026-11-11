use crate::bus::{ControlTx, LifecycleTx, Stream};
use crate::orchestrator::events::{UiRequest, UiRequestResponse, WorkerCommand};
use crate::orchestrator::{AgentSwitchHook, turn_outcome::TurnResult};
use crate::protocol::McpManagement;
use crate::protocol::contracts::CoreRuntime;
use crate::protocol::model_switching::ModelSwitching;
use crate::session::management::{SessionPersistence, SessionState};
use agnt_kernel::bus::{ControlCommand, LifecycleEvent};
use agnt_kernel::{
    CompactionRecord, PromptRecord, PromptStamps, SessionId, SessionLifecycle, SessionRecord,
};

use tokio::sync::mpsc;

/// The bus concerns a dispatched command publishes to.
///
/// Named fields, one per concern the dispatcher writes: the never-dropped
/// control intake, the ordered lifecycle spine, and the ordered record stream.
/// The worker builds one from the bus it was handed, so the dispatcher never
/// sees the hub.
pub struct DispatchSources {
    pub control: ControlTx,
    pub lifecycle: LifecycleTx,
    pub stream: Stream,
}

/// Dispatches [`WorkerCommand`] variants to a runtime that implements all
/// focused capability traits.
pub struct CommandRouter;

impl CommandRouter {
    /// Dispatch a single [`WorkerCommand`] to the runtime.
    ///
    /// Returns `true` if the worker loop should continue, `false` if it should
    /// shut down (i.e. `WorkerCommand::Shutdown`).
    ///
    /// `on_agent_switch` is an optional hook invoked after a successful
    /// agent switch, receiving the new agent's identity, description, and icon.
    pub async fn dispatch<R, H>(
        cmd: WorkerCommand,
        runtime: &mut R,
        result_tx: &mpsc::Sender<TurnResult>,
        on_agent_switch: Option<H>,
        sources: &DispatchSources,
    ) -> bool
    where
        R: CoreRuntime + McpManagement + ModelSwitching + SessionState + SessionPersistence + Send,
        H: AgentSwitchHook,
    {
        match cmd {
            WorkerCommand::ExecuteTurn {
                prompt,
                span,
                task_id,
            } => {
                log::debug!("Router: ExecuteTurn prompt_len={}", prompt.len());
                // The turn's own spine rows are published by the executor that
                // runs it, at its entry and its two exhaustive exits, so one
                // publisher brackets the turn and the attribution travels with
                // the command. The result carries the same id back, so the
                // consumer publishes the completion for the task this turn ran.
                let turn_task_id = task_id.clone();
                let end = runtime.execute_turn(prompt, None, span, task_id).await;
                let outcome = TurnResult::from_end(end).with_task_id(turn_task_id);
                let _ = result_tx.send(outcome).await;
                true
            }
            WorkerCommand::RunCompaction { source } => {
                log::trace!("Router: RunCompaction source={source:?}");
                if let Err(e) = runtime.run_compaction(source).await {
                    let _ = sources.stream.publish(SessionRecord::Compaction(
                        CompactionRecord::Failed {
                            source,
                            message: e.clone(),
                        },
                    ));
                    let _ = sources
                        .lifecycle
                        .publish(LifecycleEvent::CompactionFailed { source, message: e });
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
            WorkerCommand::AttachA2aContext {
                context_id,
                prompt,
                task_id,
            } => {
                let prefix = crate::session::prefix::dir_prefix(runtime.cwd());
                let key = crate::orchestrator::external::external_session_key(&prefix, &context_id);
                log::info!("Router: AttachA2aContext context_id={context_id} key={key}");
                // Same-session attach: the transcript already shows this
                // session. Skip the clear/hydrate cycle so the incoming prompt
                // does not wipe the visible history.
                if runtime.current_session_id() == Some(key.as_str()) {
                    Self::publish_external_prompt(runtime, sources, task_id, prompt);
                    return true;
                }
                match runtime.attach_session(&key).await {
                    Ok(snapshots) => {
                        // The switch marker first, then the stored records: the
                        // surface derives the clear from the marker and folds
                        // everything after it as the new transcript.
                        let _ = sources.stream.publish(SessionRecord::Session(
                            SessionLifecycle::Switched {
                                from: None,
                                to: SessionId::new(key.clone()),
                            },
                        ));
                        let (records, last_total_tokens) =
                            crate::protocol::contracts::project_snapshots(snapshots);
                        for record in records {
                            let _ = sources.stream.publish(record);
                        }
                        // The switch replaces the transcript the context window
                        // is measured from, so the fresh session starts from the
                        // count its stored rows report rather than from the
                        // previous session's last total.
                        runtime.seed_last_total_tokens(last_total_tokens);
                        let _ = sources.lifecycle.publish(LifecycleEvent::SessionSwitched {
                            from: None,
                            to: key,
                        });
                    }
                    Err(e) => {
                        log::warn!("Failed to attach A2A context session '{key}': {e}");
                        // The prompt belongs to a session that does not exist,
                        // so nothing is enqueued into the one that happens to
                        // be current. The caller moved the task to `Working` at
                        // the boundary and waits for a terminal state, so the
                        // failure travels the spine every task outcome uses and
                        // the write-back listener transitions the store.
                        let _ = sources.lifecycle.publish(LifecycleEvent::TaskFailed {
                            task_id,
                            error: e.to_string(),
                        });
                        return true;
                    }
                }
                // Enqueue the incoming message only after the transcript has
                // been replaced, so it never renders above the previous
                // session's history.
                Self::publish_external_prompt(runtime, sources, task_id, prompt);
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
    pub async fn dispatch_ui_request<R, H>(
        request: UiRequest,
        runtime: &mut R,
        on_agent_switch: &Option<H>,
        response_tx: mpsc::Sender<UiRequestResponse>,
    ) where
        R: CoreRuntime + McpManagement + ModelSwitching + SessionState + SessionPersistence + Send,
        H: AgentSwitchHook,
    {
        match request {
            UiRequest::SwitchModel { spec } => {
                log::debug!("Router: UiRequest SwitchModel spec={spec}");
                let result = runtime.switch_model(&spec).await;
                let _ = response_tx
                    .send(UiRequestResponse::ModelSwitch(result))
                    .await;
            }
            UiRequest::SwitchAgent { name } => {
                log::debug!("Router: UiRequest SwitchAgent name={name}");
                let result = runtime.switch_agent(&name).await;
                let response = result.map(|agent_identity| {
                    let model_identity = runtime.active_model_identity();
                    let max_tokens = runtime.max_context_tokens();
                    let icon = runtime.agent_icon().map(|s| s.to_string());
                    if let Some(cb) = on_agent_switch {
                        let description = runtime.agent_description().map(|s| s.to_string());
                        cb.on_agent_switch(agent_identity.clone(), description, icon.clone());
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
                // The store may resolve a partial id to a longer stored one, and
                // the runtime attaches to that. The confirmation names the
                // session the runtime is on — the picker's omission is built from
                // this id — while a failed load leaves the runtime where it was,
                // so the request's own id travels back with the failure.
                let id = match &result {
                    Ok(_) => runtime
                        .current_session_id()
                        .map(str::to_string)
                        .unwrap_or(id),
                    Err(_) => id,
                };
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
            // A picker refresh is answered by the loop's catalog reader, never by
            // the worker: the surface's hand is answered while the turn runs, so
            // a refresh reaching the worker is stale surface state.
            UiRequest::RefreshSessionPicker { .. } | UiRequest::RefreshAgentPicker { .. } => {
                log::debug!("Router: UiRequest picker refresh ignored");
            }
        }
    }

    /// Publish the prompt row an arrival draws and the command the loop admits
    /// it by.
    ///
    /// The attach branch is where the runtime's hand exists, so the row's
    /// footer facts are read here: the instant this arrival reached the router,
    /// the model identity active at it, and the persona active at it — the same
    /// three facts a surface submit stamps.
    fn publish_external_prompt<R>(
        runtime: &R,
        sources: &DispatchSources,
        task_id: String,
        prompt: String,
    ) where
        R: ModelSwitching,
    {
        let millis = chrono::Utc::now().timestamp_millis();
        let _ = sources.stream.publish(SessionRecord::Prompt(PromptRecord {
            text: prompt.clone(),
            stamps: PromptStamps {
                millis: Some(millis),
                model: Some(runtime.active_model_identity()),
                agent: runtime.active_agent_identity().map(str::to_string),
            },
            task_id: Some(task_id.clone()),
        }));
        let _ = sources
            .control
            .publish(ControlCommand::EnqueueExternalPrompt { task_id, prompt });
    }
}

#[cfg(test)]
#[path = "../../test/orchestrator/router.rs"]
mod router_test;
