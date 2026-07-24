use crate::orchestrator::{WorkerCommand, turn_outcome::TurnOutcome};
use crate::protocol::{
    compaction::CompactionTriggerDecision,
    compaction_runtime::HasCompaction,
    contracts::{CoreRuntime, ProgressUi},
    mcp_management::HasMcpManagement,
    model_switching::HasModelSwitching,
    session_management::HasSessionManagement,
};

use tokio::sync::mpsc;

const COMPACTION_FAILURE_WARNING: &str =
    "Session compaction failed: sliding_summary summarization unavailable";

/// Dispatches [`WorkerCommand`] variants to a runtime that implements all
/// focused capability traits.
///
/// This struct encapsulates the worker-thread dispatch logic that was previously
/// inline inside `run_interactive_loop`. Each match arm is identical to the
/// original orchestrator implementation — this is a pure extraction.
pub struct CommandRouter;

impl CommandRouter {
    /// Dispatch a single [`WorkerCommand`] to the runtime.
    ///
    /// Returns `true` if the worker loop should continue, `false` if it should
    /// shut down (i.e. `WorkerCommand::Shutdown`).
    pub async fn dispatch<R, U>(
        cmd: WorkerCommand,
        runtime: &mut R,
        ui: &mut U,
        result_tx: &mpsc::Sender<TurnOutcome>,
    ) -> bool
    where
        R: CoreRuntime
            + HasMcpManagement
            + HasModelSwitching
            + HasSessionManagement
            + HasCompaction
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
                    (agent_identity, model_identity, max_tokens)
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
