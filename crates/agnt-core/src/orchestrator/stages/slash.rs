use crate::bus::CancelEvent;
use crate::orchestrator::stages::{OrchestrationContext, SlashHandler};
use crate::orchestrator::{UiStateEvent, WorkerCommand};
use crate::protocol::contracts::SharedUiAction;
use crate::protocol::events::{SystemEvent, SystemKind};
use crate::protocol::slash::{SlashCommand, SlashParseResult, parse_slash_command};

/// Processes slash commands and regular prompt submissions from the UI.
///
/// Only processes prompts when the worker is idle.
#[derive(Default)]
pub(crate) struct SlashStage;

impl SlashHandler for SlashStage {
    async fn handle(
        &mut self,
        prompt: String,
        task_id: Option<String>,
        ctx: &mut OrchestrationContext<'_>,
    ) {
        if prompt.trim().is_empty() {
            return;
        }

        match parse_slash_command(&prompt) {
            SlashParseResult::Command(SlashCommand::Compact) => {
                // Fire a compaction request on the bus. The orchestrator (the
                // single subscriber) receives it and dispatches compaction to
                // the worker asynchronously.
                let _ = ctx
                    .bus
                    .compaction()
                    .send(SystemEvent::CompactionRequested {
                        source: "slash".to_string(),
                    })
                    .await;
            }
            SlashParseResult::Command(SlashCommand::New) => {
                let _ = ctx.worker_tx.send(WorkerCommand::NewSession).await;
                let _ = ctx.bus.ui_state().send(UiStateEvent::ClearTranscript).await;
                let _ = ctx.bus.ui_state().send(UiStateEvent::PushStartupLogo).await;
            }
            SlashParseResult::Command(SlashCommand::Help) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Help))
                    .await;
            }
            SlashParseResult::Command(SlashCommand::Status) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Status))
                    .await;
            }
            SlashParseResult::Command(SlashCommand::Mcp) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Mcps))
                    .await;
            }
            SlashParseResult::Command(SlashCommand::Models) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Models))
                    .await;
            }
            SlashParseResult::Command(SlashCommand::Agent) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Agents))
                    .await;
            }
            SlashParseResult::Command(SlashCommand::Session) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(
                        SharedUiAction::Sessions,
                    ))
                    .await;
            }
            SlashParseResult::Command(SlashCommand::Theme) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Themes))
                    .await;
            }
            SlashParseResult::Command(SlashCommand::Skills) => {
                let _ = ctx
                    .bus
                    .ui_state()
                    .send(UiStateEvent::ExecuteSharedUiAction(SharedUiAction::Skills))
                    .await;
            }
            SlashParseResult::Unknown(command) => {
                let _ = ctx
                    .bus
                    .ui_event()
                    .send(SystemEvent::SystemMessage {
                        kind: SystemKind::Warning,
                        text: format!("Unknown slash command: {command}"),
                    })
                    .await;
            }
            SlashParseResult::NotSlash => {
                // Regular prompt: dispatch to worker. The caller supplies the
                // task id of the A2A task that queued this prompt, so the turn
                // is attributed to that task instead of a user submit.
                if let Some(task_id) = task_id.as_deref() {
                    *ctx.active_external_prompt = Some(prompt.clone());
                    *ctx.active_external_task_id = Some(task_id.to_string());
                    if ctx.pending_external_cancel.as_deref() == Some(task_id) {
                        *ctx.pending_external_cancel = None;
                        let _ = ctx.bus.cancel().send(CancelEvent::Requested).await;
                    }
                }
                // Control-plane turn-start event is published by the worker
                // router, which owns the turn dispatch and the attribution.
                match ctx
                    .worker_tx
                    .send(WorkerCommand::ExecuteTurn {
                        prompt,
                        span: ctx.span,
                        task_id,
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
