use std::sync::mpsc as std_mpsc;

use nu_protocol::LabeledError;

use crate::orchestrator::stages::{OrchestrationContext, StageOutcome};
use crate::orchestrator::{PendingCompactionTrigger, WorkerCommand};
use crate::protocol::compaction::CompactionTriggerSource;
use crate::protocol::contracts::{
    DisplayStateUi, LifecycleUi, ProgressUi, SharedUiAction, TranscriptUi, UserInputUi,
};
use crate::protocol::event::UiEvent;
use crate::protocol::slash::{SlashCommand, SlashParseResult, parse_slash_command};

/// Processes slash commands and regular prompt submissions from the UI.
///
/// Only processes prompts when the worker is idle.
pub(crate) struct SlashStage {
    /// Set when a `/compact` is dispatched; picked up by `CompactionStage` via
    /// `OrchestratorStages::poll_all` after each poll.
    pending_compaction_trigger: Option<PendingCompactionTrigger>,
}

impl SlashStage {
    pub fn new() -> Self {
        Self {
            pending_compaction_trigger: None,
        }
    }

    /// Take the pending compaction trigger, if any, produced during `poll`.
    /// Called by `OrchestratorStages::poll_all` to hand the receiver to `CompactionStage`.
    pub fn take_pending_compaction_trigger(&mut self) -> Option<PendingCompactionTrigger> {
        self.pending_compaction_trigger.take()
    }

    pub async fn poll<U>(&mut self, ctx: &mut OrchestrationContext<'_, U>) -> StageOutcome
    where
        U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
    {
        // Only process prompts when the worker is idle
        if *ctx.worker_active {
            return StageOutcome::Idle;
        }

        let mut handled = false;

        while let Some(prompt) = ctx.ui.take_submitted_prompt() {
            if prompt.trim().is_empty() {
                continue;
            }

            match parse_slash_command(&prompt) {
                SlashParseResult::Command(SlashCommand::Compact) => {
                    let (response_tx, response_rx) = std_mpsc::channel();
                    if ctx
                        .worker_tx
                        .send(WorkerCommand::ExecuteCompactionTrigger {
                            source: CompactionTriggerSource::SlashCompact,
                            response_tx,
                        })
                        .await
                        .is_ok()
                    {
                        self.pending_compaction_trigger = Some(response_rx);
                    } else {
                        ctx.ui.emit(&UiEvent::Warning {
                            message: "Compaction worker channel closed".to_string(),
                        });
                    }
                    handled = true;
                    continue;
                }
                SlashParseResult::Command(SlashCommand::New) => {
                    let _ = ctx.worker_tx.send(WorkerCommand::NewSession).await;
                    ctx.ui.clear_transcript();
                    handled = true;
                    continue;
                }
                SlashParseResult::Command(SlashCommand::Help) => {
                    let _ = ctx.ui.execute_shared_ui_action(SharedUiAction::Help);
                    handled = true;
                    continue;
                }
                SlashParseResult::Command(SlashCommand::Status) => {
                    let _ = ctx.ui.execute_shared_ui_action(SharedUiAction::Status);
                    handled = true;
                    continue;
                }
                SlashParseResult::Command(SlashCommand::Mcp) => {
                    let _ = ctx.ui.execute_shared_ui_action(SharedUiAction::Mcps);
                    handled = true;
                    continue;
                }
                SlashParseResult::Command(SlashCommand::Models) => {
                    let _ = ctx.ui.execute_shared_ui_action(SharedUiAction::Models);
                    handled = true;
                    continue;
                }
                SlashParseResult::Command(SlashCommand::Agent) => {
                    let _ = ctx.ui.execute_shared_ui_action(SharedUiAction::Agents);
                    handled = true;
                    continue;
                }
                SlashParseResult::Command(SlashCommand::Session) => {
                    let _ = ctx.ui.execute_shared_ui_action(SharedUiAction::Sessions);
                    handled = true;
                    continue;
                }
                SlashParseResult::Command(SlashCommand::Theme) => {
                    let _ = ctx.ui.execute_shared_ui_action(SharedUiAction::Themes);
                    handled = true;
                    continue;
                }
                SlashParseResult::Unknown(command) => {
                    ctx.ui.emit(&UiEvent::Warning {
                        message: format!("Unknown slash command: {command}"),
                    });
                    handled = true;
                    continue;
                }
                SlashParseResult::NotSlash => {}
            }

            // Regular prompt: dispatch to worker
            match ctx
                .worker_tx
                .send(WorkerCommand::ExecuteTurn {
                    prompt,
                    span: ctx.span,
                })
                .await
            {
                Ok(()) => {
                    *ctx.worker_active = true;
                    handled = true;
                }
                Err(_) => {
                    return StageOutcome::Fatal(LabeledError::new(
                        "Interactive worker channel closed unexpectedly",
                    ));
                }
            }
            // Break regardless: either we dispatched (worker now active) or channel closed.
            break;
        }

        if handled {
            StageOutcome::Handled
        } else {
            StageOutcome::Idle
        }
    }
}
