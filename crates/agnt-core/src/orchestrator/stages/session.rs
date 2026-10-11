use crate::bus::TurnEvent;
use crate::orchestrator::stages::{OrchestrationContext, SessionHandler};
use crate::orchestrator::turn_outcome::TurnOutcome;
use crate::protocol::event::UiEvent;
use crate::utils::value_ext::extract_response_text_from_value;

/// Applies turn outcomes (success, cancel, error).
#[derive(Default)]
pub(crate) struct SessionStage;

/// Reason published with `TurnEvent::TaskFailed` when an external turn is
/// cancelled before it produces a result.
pub const TASK_CANCELLED_NOTICE: &str = "Task cancelled before completion";

impl SessionStage {
    /// Publish `TurnEvent::TaskFailed` for the active external task, if any.
    ///
    /// Mirrors the `TaskCompleted` publication: the event stays on
    /// `bus.turn()` (control plane) and is not rendered in the TUI. Consumes
    /// `active_external_prompt` and `active_external_task_id` so a later
    /// cancel signal for the same task is ignored.
    async fn publish_task_failed(&self, error: String, ctx: &mut OrchestrationContext<'_>) {
        if ctx.active_external_prompt.take().is_none() {
            return;
        }
        let Some(task_id) = ctx.active_external_task_id.take() else {
            return;
        };
        let _ = ctx
            .bus
            .turn()
            .send(TurnEvent::TaskFailed { task_id, error })
            .await;
    }
}

impl SessionHandler for SessionStage {
    async fn handle_outcome(&mut self, outcome: TurnOutcome, ctx: &mut OrchestrationContext<'_>) {
        *ctx.worker_active = false;
        match outcome {
            TurnOutcome::Success(value) => {
                log::info!("Turn outcome: Success");

                // Publish a turn-completion event if this turn was triggered
                // by an external prompt (e.g., A2A task). This is a control-plane
                // event: it stays on `bus.turn()` and is not rendered in the TUI.
                if ctx.active_external_prompt.take().is_some() {
                    let response_text = extract_response_text_from_value(&value);
                    let task_id = ctx.active_external_task_id.take();
                    if let Some(task_id) = task_id {
                        let _ = ctx
                            .bus
                            .turn()
                            .send(TurnEvent::TaskCompleted {
                                output: response_text,
                                task_id,
                            })
                            .await;
                    }
                }
            }
            TurnOutcome::Cancelled => {
                log::info!("Turn outcome: Cancelled");
                // Fail the external task — the turn didn't complete.
                self.publish_task_failed(TASK_CANCELLED_NOTICE.to_string(), ctx)
                    .await;
            }
            TurnOutcome::Error(error) => {
                log::warn!(
                    "Turn outcome: Error msg={}",
                    &error.msg[..error.msg.len().min(200)]
                );
                // Fail the external task before clearing the ids — the A2A
                // listener needs the task id to transition the task.
                self.publish_task_failed(error.msg.clone(), ctx).await;
                let _ = ctx
                    .bus
                    .ui_event()
                    .send(UiEvent::TurnError { message: error.msg })
                    .await;
            }
        }
    }
}
