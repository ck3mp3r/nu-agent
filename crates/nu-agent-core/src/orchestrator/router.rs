use crate::orchestrator::{WorkerCommand, turn_outcome::TurnOutcome};
use crate::protocol::{
    compaction::CompactionTriggerDecision,
    contracts::{ExtendedRuntime, ProgressUi},
};

use std::sync::mpsc;

const COMPACTION_FAILURE_WARNING: &str =
    "Session compaction failed: sliding_summary summarization unavailable";

/// Dispatches [`WorkerCommand`] variants to an [`ExtendedRuntime`].
///
/// This struct encapsulates the worker-thread dispatch logic that was previously
/// inline inside `run_interactive_loop`. Each match arm is identical to the
/// original orchestrator implementation — this is a pure extraction.
pub struct CommandRouter<'a, R: ExtendedRuntime> {
    runtime: &'a mut R,
}

impl<'a, R: ExtendedRuntime + Send> CommandRouter<'a, R> {
    pub fn new(runtime: &'a mut R) -> Self {
        Self { runtime }
    }

    /// Dispatch a single [`WorkerCommand`] to the runtime.
    ///
    /// Returns `true` if the worker loop should continue, `false` if it should
    /// shut down (i.e. `WorkerCommand::Shutdown`).
    pub fn dispatch<U: ProgressUi>(
        &mut self,
        cmd: WorkerCommand,
        ui: &mut U,
        result_tx: &mpsc::Sender<TurnOutcome>,
    ) -> bool {
        match cmd {
            WorkerCommand::ExecuteTurn { prompt, span } => {
                let result = self.runtime.execute_turn(ui, prompt, None, span);
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
                let _ = result_tx.send(outcome);
                true
            }
            WorkerCommand::EvaluateAutoCompaction { response_tx } => {
                let warning = match self.runtime.evaluate_auto_compaction() {
                    Some(CompactionTriggerDecision::Fire { source, .. }) => self
                        .runtime
                        .execute_compaction_trigger(ui, source)
                        .err()
                        .map(|_error| COMPACTION_FAILURE_WARNING.to_string()),
                    _ => None,
                };
                let _ = response_tx.send(warning);
                true
            }
            WorkerCommand::ExecuteCompactionTrigger {
                source,
                response_tx,
            } => {
                let warning = self
                    .runtime
                    .execute_compaction_trigger(ui, source)
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
                let result = self.runtime.set_mcp_server_enabled(&server_name, enable);
                let visible_count = self.runtime.llm_visible_mcp_tool_count();
                let visible_count_for_server = self
                    .runtime
                    .llm_visible_mcp_tool_count_for_server(&server_name);
                let visible_names_by_server = self.runtime.llm_visible_mcp_tool_names_by_server();
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
                let _ = response_tx.send(self.runtime.switch_model(&model_spec));
                true
            }
            WorkerCommand::SwitchAgent {
                agent_name,
                response_tx,
            } => {
                let result = self.runtime.switch_agent(&agent_name);
                let response = result.map(|agent_identity| {
                    let model_identity = self.runtime.active_model_identity();
                    (agent_identity, model_identity)
                });
                let _ = response_tx.send(response);
                true
            }
            WorkerCommand::ClearSession => {
                self.runtime.clear_session();
                true
            }
            WorkerCommand::Shutdown => false,
        }
    }
}

#[cfg(test)]
#[path = "router_test.rs"]
mod router_test;
