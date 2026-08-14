use tokio::sync::mpsc;

use crate::bus::{TurnEvent, WarningEvent};
use crate::orchestrator::stages::{OrchestrationContext, StageOutcome};
use crate::orchestrator::turn_outcome::TurnOutcome;
use crate::protocol::contracts::{
    DisplayStateUi, LifecycleUi, ProgressUi, TranscriptUi, UserInputUi,
};
use crate::utils::value_ext::extract_response_text_from_value;

/// Polls the worker result channel and applies turn outcomes (success, cancel, error).
pub(crate) struct SessionStage {
    worker_result_rx: mpsc::Receiver<TurnOutcome>,
}

impl SessionStage {
    pub fn new(worker_result_rx: mpsc::Receiver<TurnOutcome>) -> Self {
        Self { worker_result_rx }
    }

    pub fn poll<U>(&mut self, ctx: &mut OrchestrationContext<'_, U>) -> StageOutcome
    where
        U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
    {
        let mut handled = false;

        while let Ok(outcome) = self.worker_result_rx.try_recv() {
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
            handled = true;
        }

        if handled {
            StageOutcome::Handled
        } else {
            StageOutcome::Idle
        }
    }
}
