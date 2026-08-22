use crate::bus::{TurnEvent, WarningEvent};
use crate::orchestrator::stages::{OrchestrationContext, SessionHandler};
use crate::orchestrator::turn_outcome::TurnOutcome;
use crate::utils::value_ext::extract_response_text_from_value;

/// Applies turn outcomes (success, cancel, error).
pub(crate) struct SessionStage;

impl SessionStage {
    pub fn new() -> Self {
        Self
    }
}

impl SessionHandler for SessionStage {
    fn handle_outcome(&mut self, outcome: TurnOutcome, ctx: &mut OrchestrationContext) {
        *ctx.worker_active = false;
        *ctx.should_evaluate_compaction = true;
        match outcome {
            TurnOutcome::Success(ref value) => {
                log::info!("Turn outcome: Success");

                // Publish a turn-completion event if this turn was triggered
                // by an external prompt (e.g., A2A task).
                if ctx.active_external_prompt.take().is_some() {
                    let response_text = extract_response_text_from_value(value);
                    let task_id = ctx.active_external_task_id.take();
                    if let Some(task_id) = task_id {
                        let _ = ctx.bus.turn().send(TurnEvent::TaskCompleted {
                            output: response_text,
                            task_id,
                        });
                    }
                }
            }
            TurnOutcome::Cancelled => {
                log::info!("Turn outcome: Cancelled");
                // Clear pending external prompt — the turn didn't complete.
                let _ = ctx.active_external_prompt.take();
                let _ = ctx.active_external_task_id.take();
            }
            TurnOutcome::Error(error) => {
                log::warn!(
                    "Turn outcome: Error msg={}",
                    &error.msg[..error.msg.len().min(200)]
                );
                let _ = ctx.bus.warning().send(WarningEvent::TurnError {
                    message: format!("Turn failed: {}", error.msg),
                });
                // Clear pending external prompt — the turn didn't complete.
                let _ = ctx.active_external_prompt.take();
                let _ = ctx.active_external_task_id.take();
            }
        }
    }
}
