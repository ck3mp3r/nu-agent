use agnt_kernel::bus::{
    ControlCommand, LifecycleEvent, NarrationEvent, PromptStamps, SharedUiAction,
};
use agnt_kernel::{CompactionSource, NoticeKind};

use crate::orchestrator::events::WorkerCommand;
use crate::orchestrator::stages::{OrchestrationContext, SlashHandler};
use crate::protocol::slash::{SlashCommand, SlashParseResult, parse_slash_command};

/// Processes slash commands and regular prompt submissions from the UI.
///
/// The stage handles every submission whatever the worker is doing: the
/// commands act at once, while an effect the worker performs (`/new`'s mint)
/// applies only after a running turn finishes, so the deferral is announced.
#[derive(Default)]
pub(crate) struct SlashStage;

impl SlashStage {
    /// Ask a surface to open one of its shared panels.
    ///
    /// The request is a control command: the surface that receives it owns what
    /// the panel looks like and how it opens. This stage only names the verb.
    fn request_panel(&self, action: SharedUiAction, ctx: &mut OrchestrationContext<'_>) {
        let _ = ctx
            .control
            .publish(ControlCommand::SharedUiAction { action });
    }
}

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
                // A compaction request is a command to the loop owner: it
                // dispatches the sweep to the worker asynchronously.
                let _ = ctx.control.publish(ControlCommand::CompactionRequested {
                    source: CompactionSource::Manual,
                });
            }
            SlashParseResult::Command(SlashCommand::Uncompact) => {
                // An uncompaction is the mirror image: the loop owner dispatches
                // the undo to the worker asynchronously, like a compaction.
                let _ = ctx.control.publish(ControlCommand::UncompactRequested {
                    source: CompactionSource::Manual,
                });
            }
            SlashParseResult::Command(SlashCommand::New) => {
                // The mint happens in the worker: `new_session()` publishes the
                // session's own `Started` record and spine row, so the
                // transcript clear derives from the record rather than from a
                // command here. The worker serves its channel in order, so a
                // mint queued behind a running turn applies only when that turn
                // ends — the deferral is announced where the command arrives.
                if *ctx.worker_active {
                    let _ = ctx.narration.publish(NarrationEvent::SystemMessage {
                        kind: NoticeKind::Warning,
                        text: "/new applies after the current turn finishes".to_string(),
                    });
                }
                let _ = ctx.worker_tx.send(WorkerCommand::NewSession).await;
            }
            SlashParseResult::Command(SlashCommand::Help) => {
                self.request_panel(SharedUiAction::Help, ctx);
            }
            SlashParseResult::Command(SlashCommand::Status) => {
                self.request_panel(SharedUiAction::Status, ctx);
            }
            SlashParseResult::Command(SlashCommand::Mcp) => {
                self.request_panel(SharedUiAction::Mcps, ctx);
            }
            SlashParseResult::Command(SlashCommand::Models) => {
                self.request_panel(SharedUiAction::Models, ctx);
            }
            SlashParseResult::Command(SlashCommand::Agent) => {
                self.request_panel(SharedUiAction::Agents, ctx);
            }
            SlashParseResult::Command(SlashCommand::Session) => {
                self.request_panel(SharedUiAction::Sessions, ctx);
            }
            SlashParseResult::Command(SlashCommand::Theme) => {
                self.request_panel(SharedUiAction::Themes, ctx);
            }
            SlashParseResult::Command(SlashCommand::Skills) => {
                self.request_panel(SharedUiAction::Skills, ctx);
            }
            SlashParseResult::Unknown(command) => {
                let _ = ctx.narration.publish(NarrationEvent::SystemMessage {
                    kind: NoticeKind::Warning,
                    text: format!("Unknown slash command: {command}"),
                });
            }
            SlashParseResult::NotSlash => {
                // The intake row: this stage is where a prompt is accepted, so
                // the spine's `PromptSubmitted` is published here, once, for
                // every prompt. The stamps are what this boundary knows — a
                // prompt that reaches the runtime through the surface carries
                // no footer facts, so the record states absence.
                let _ = ctx.lifecycle.publish(LifecycleEvent::PromptSubmitted {
                    text: prompt.clone(),
                    task_id: task_id.clone(),
                    stamps: PromptStamps::default(),
                });
                // Regular prompt: dispatch to worker. The caller supplies the
                // task id of the A2A task whose prompt this is, so the turn is
                // attributed to that task instead of a user submit. The id rides
                // the command into the turn and back out on its result.
                //
                // The turn's own spine rows are published by the executor that
                // runs it, so the attribution travels with the command.
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
