use agnt_kernel::NoticeKind;
use agnt_kernel::bus::{LifecycleEvent, NarrationEvent};

use crate::orchestrator::stages::{OrchestrationContext, SessionHandler};
use crate::orchestrator::turn_outcome::{TurnOutcome, TurnResult};
use crate::utils::value_ext::extract_response_text_from_value;

/// Applies turn outcomes (success, cancel, error).
#[derive(Default)]
pub(crate) struct SessionStage;

/// Reason published with `LifecycleEvent::TaskFailed` when an external turn is
/// cancelled before it produces a result.
pub const TASK_CANCELLED_NOTICE: &str = "Task cancelled before completion";

impl SessionStage {
    /// Publish `LifecycleEvent::TaskFailed` for the task this turn ran, if any.
    ///
    /// Mirrors the `TaskCompleted` publication: the event stays on
    /// `bus.lifecycle()` and is not rendered in the TUI.
    async fn publish_task_failed(
        &self,
        error: String,
        task_id: Option<String>,
        ctx: &mut OrchestrationContext<'_>,
    ) {
        let Some(task_id) = task_id else {
            return;
        };
        let _ = ctx
            .lifecycle
            .publish(LifecycleEvent::TaskFailed { task_id, error });
    }
}

impl SessionHandler for SessionStage {
    async fn handle_outcome(&mut self, outcome: TurnResult, ctx: &mut OrchestrationContext<'_>) {
        *ctx.worker_active = false;
        let TurnResult { outcome, task_id } = outcome;
        match outcome {
            TurnOutcome::Success(value) => {
                log::info!("Turn outcome: Success");

                // Publish a turn-completion event for the task whose prompt
                // this turn ran. This is a lifecycle event: it stays on
                // `bus.lifecycle()` and is not rendered in the TUI. The id
                // rides the turn's own result, so a turn dispatched while this
                // one ran cannot re-route this completion to another task.
                if let Some(task_id) = task_id {
                    let response_text = extract_response_text_from_value(&value);
                    let _ = ctx.lifecycle.publish(LifecycleEvent::TaskCompleted {
                        output: response_text,
                        task_id,
                    });
                }
            }
            TurnOutcome::Cancelled => {
                log::info!("Turn outcome: Cancelled");
                // Fail the external task — the turn didn't complete.
                self.publish_task_failed(TASK_CANCELLED_NOTICE.to_string(), task_id, ctx)
                    .await;
            }
            TurnOutcome::Error(error) => {
                log::warn!(
                    "Turn outcome: Error msg={}",
                    &error.msg[..error.msg.len().min(200)]
                );
                // Fail the external task the turn ran for — the A2A listener
                // needs the task id to transition the task.
                self.publish_task_failed(error.msg.clone(), task_id, ctx)
                    .await;
                let _ = ctx.narration.publish(NarrationEvent::SystemMessage {
                    kind: NoticeKind::TurnError,
                    text: error.msg,
                });
            }
        }
    }
}
