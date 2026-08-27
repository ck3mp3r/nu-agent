use crate::bus::{CompactionEvent, TurnEvent, WarningEvent};
use crate::orchestrator::stages::{OrchestrationContext, SlashHandler};
use crate::orchestrator::{UiStateEvent, WorkerCommand};
use crate::protocol::contracts::SharedUiAction;
use crate::protocol::slash::{SlashCommand, SlashParseResult, parse_slash_command};

/// Processes slash commands and regular prompt submissions from the UI.
///
/// Only processes prompts when the worker is idle.
pub(crate) struct SlashStage;

impl SlashStage {
    pub fn new() -> Self {
        Self
    }
}

impl SlashHandler for SlashStage {
    async fn handle(&mut self, prompt: String, ctx: &mut OrchestrationContext<'_>) {
        if prompt.trim().is_empty() {
            return;
        }

        match parse_slash_command(&prompt) {
            SlashParseResult::Command(SlashCommand::Compact) => {
                // Fire a compaction request on the bus. The orchestrator (the
                // single subscriber) receives it and dispatches compaction to
                // the worker asynchronously.
                let _ = ctx.bus.compaction().send(CompactionEvent::Requested {
                    source: "slash".to_string(),
                });
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
}
