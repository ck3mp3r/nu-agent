use crate::orchestrator::{OnAgentSwitch, WorkerCommand, turn_outcome::TurnOutcome};
use crate::protocol::{
    compaction::CompactionTriggerDecision,
    compaction_runtime::Compaction,
    contracts::{CoreRuntime, ProgressUi},
    mcp_management::McpManagement,
    model_switching::ModelSwitching,
    session_management::{SessionPersistence, SessionState},
};

use tokio::sync::mpsc;

const COMPACTION_FAILURE_WARNING: &str =
    "Session compaction failed: sliding_summary summarization unavailable";

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
    pub async fn dispatch<R, U>(
        cmd: WorkerCommand,
        runtime: &mut R,
        ui: &mut U,
        result_tx: &mpsc::Sender<TurnOutcome>,
        on_agent_switch: Option<OnAgentSwitch>,
    ) -> bool
    where
        R: CoreRuntime
            + McpManagement
            + ModelSwitching
            + SessionState
            + SessionPersistence
            + Compaction
            + Send,
        U: ProgressUi + Send,
    {
        match cmd {
            WorkerCommand::ExecuteTurn { prompt, span } => {
                log::debug!("Router: ExecuteTurn prompt_len={}", prompt.len());
                let result = runtime.execute_turn(ui, prompt, None, span).await;
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
            WorkerCommand::EvaluateAutoCompaction { response_tx } => {
                log::trace!("Router: EvaluateAutoCompaction");
                let warning = match runtime.evaluate_auto_compaction() {
                    Some(CompactionTriggerDecision::Fire { source, .. }) => {
                        log::trace!("Auto-compaction firing: source={source:?}");
                        runtime
                            .execute_compaction_trigger(ui, source)
                            .await
                            .err()
                            .map(|_error| COMPACTION_FAILURE_WARNING.to_string())
                    }
                    _ => None,
                };
                let _ = response_tx.send(warning);
                true
            }
            WorkerCommand::ExecuteCompactionTrigger {
                source,
                response_tx,
            } => {
                log::trace!("Router: ExecuteCompactionTrigger source={source:?}");
                let warning = runtime
                    .execute_compaction_trigger(ui, source)
                    .await
                    .err()
                    .map(|_error| COMPACTION_FAILURE_WARNING.to_string());
                let _ = response_tx.send(warning);
                true
            }
            WorkerCommand::ToggleMcp {
                server_name,
                enable,
                response_tx,
            } => {
                log::debug!("Router dispatching ToggleMcp: server={server_name} enable={enable}");
                let result = runtime.set_mcp_server_enabled(&server_name, enable).await;
                let visible_count = runtime.llm_visible_mcp_tool_count();
                let visible_count_for_server =
                    runtime.llm_visible_mcp_tool_count_for_server(&server_name);
                let visible_names_by_server = runtime.llm_visible_mcp_tool_names_by_server();
                let success = result.is_ok();
                log::debug!(
                    "Router ToggleMcp result: server={server_name} success={success} visible_count={visible_count}"
                );
                let _ = response_tx.send((
                    result,
                    visible_count,
                    visible_count_for_server,
                    visible_names_by_server,
                ));
                true
            }
            WorkerCommand::SwitchModel {
                model_spec,
                response_tx,
            } => {
                log::debug!("Router: SwitchModel spec={model_spec}");
                let _ = response_tx.send(runtime.switch_model(&model_spec));
                true
            }
            WorkerCommand::SwitchAgent {
                agent_name,
                response_tx,
            } => {
                log::debug!("Router: SwitchAgent name={agent_name}");
                let result = runtime.switch_agent(&agent_name);
                let response = result.map(|agent_identity| {
                    let model_identity = runtime.active_model_identity();
                    let max_tokens = runtime.max_context_tokens();
                    let icon = runtime.agent_icon().map(|s| s.to_string());
                    // Notify the binary layer that the agent card should be updated.
                    if let Some(ref cb) = on_agent_switch {
                        let description = runtime.agent_description().map(|s| s.to_string());
                        cb(agent_identity.clone(), description, icon.clone());
                    }
                    (agent_identity, model_identity, max_tokens, icon)
                });
                let _ = response_tx.send(response);
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
            WorkerCommand::SwitchSession {
                session_id,
                response_tx,
            } => {
                log::info!("Router: SwitchSession id={session_id}");
                let result = runtime.load_session(&session_id).await;
                let _ = response_tx.send(result);
                true
            }
            WorkerCommand::RefreshSessionPicker { response_tx } => {
                log::debug!("Router: RefreshSessionPicker");
                let cwd = runtime.cwd();
                let result = runtime.list_sessions(cwd).await;
                let _ = response_tx.send(result);
                true
            }
            WorkerCommand::Shutdown => {
                log::info!("Router: Shutdown");
                false
            }
        }
    }
}

#[cfg(test)]
#[path = "router_test.rs"]
mod router_test;
