use crate::bus::{TurnEvent, WarningEvent};
use crate::orchestrator::stages::{OrchestrationContext, SlashHandler};
use crate::orchestrator::{PendingCompactionTrigger, UiStateEvent, WorkerCommand};
use crate::protocol::compaction::CompactionTriggerSource;
use crate::protocol::contracts::SharedUiAction;
use crate::protocol::slash::{SlashCommand, SlashParseResult, parse_slash_command};

/// Processes slash commands and regular prompt submissions from the UI.
///
/// Only processes prompts when the worker is idle.
pub(crate) struct SlashStage {
    /// Set when a `/compact` is dispatched; picked up by `CompactionStage`.
    pending_compaction_trigger: Option<PendingCompactionTrigger>,
}

impl SlashStage {
    pub fn new() -> Self {
        Self {
            pending_compaction_trigger: None,
        }
    }
}

impl SlashHandler for SlashStage {
    async fn handle(&mut self, prompt: String, ctx: &mut OrchestrationContext<'_>) {
        if prompt.trim().is_empty() {
            return;
        }

        match parse_slash_command(&prompt) {
            SlashParseResult::Command(SlashCommand::Compact) => {
                let (response_tx, response_rx) = tokio::sync::mpsc::channel(1);
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
                    let _ = ctx.bus.warning().send(WarningEvent::Message {
                        message: "Compaction worker channel closed".to_string(),
                    });
                }
            }
            SlashParseResult::Command(SlashCommand::New) => {
                let _ = ctx.worker_tx.send(WorkerCommand::NewSession).await;
                let _ = ctx.bus.ui_state().send(UiStateEvent::ClearTranscript);
                let _ = ctx.bus.ui_state().send(UiStateEvent::PushStartupLogo);
            }
            SlashParseResult::Command(SlashCommand::Help) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Help));
            }
            SlashParseResult::Command(SlashCommand::Status) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Status));
            }
            SlashParseResult::Command(SlashCommand::Mcp) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Mcps));
            }
            SlashParseResult::Command(SlashCommand::Models) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Models));
            }
            SlashParseResult::Command(SlashCommand::Agent) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Agents));
            }
            SlashParseResult::Command(SlashCommand::Session) => {
                let _ = ctx.bus.ui_state().send(UiStateEvent::ExecuteSharedUiAction(
                    SharedUiAction::Sessions,
                ));
            }
            SlashParseResult::Command(SlashCommand::Theme) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Themes));
            }
            SlashParseResult::Command(SlashCommand::Skills) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Skills));
            }
            SlashParseResult::Unknown(command) => {
                let _ = ctx.bus.warning().send(WarningEvent::Message {
                    message: format!("Unknown slash command: {command}"),
                });
            }
            SlashParseResult::NotSlash => {
                // Regular prompt: dispatch to worker
                let _ = ctx.bus.turn().send(TurnEvent::Started {
                    prompt: prompt.clone(),
                    task_id: None,
                });
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
                    }
                    Err(_) => {
                        log::error!("Interactive worker channel closed unexpectedly");
                    }
                }
            }
        }
    }

    fn take_pending_compaction_trigger(&mut self) -> Option<PendingCompactionTrigger> {
        self.pending_compaction_trigger.take()
    }
}
