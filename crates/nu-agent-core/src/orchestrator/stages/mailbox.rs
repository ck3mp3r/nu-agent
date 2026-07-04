use nu_protocol::LabeledError;

use crate::orchestrator::WorkerCommand;
use crate::orchestrator::stages::{OrchestrationContext, StageOutcome};
use crate::protocol::contracts::{
    DisplayStateUi, LifecycleUi, ProgressUi, TranscriptUi, UserInputUi,
};

/// Polls the mailbox channel for new incoming messages and drains any queued
/// mailbox prompts when the worker becomes idle.
pub(crate) struct MailboxStage {
    pending_mailbox_prompts: Vec<String>,
}

impl MailboxStage {
    pub fn new() -> Self {
        Self {
            pending_mailbox_prompts: Vec::new(),
        }
    }

    pub fn poll<U>(&mut self, ctx: &mut OrchestrationContext<'_, U>) -> StageOutcome
    where
        U: ProgressUi + UserInputUi + DisplayStateUi + LifecycleUi + TranscriptUi,
    {
        let mut handled = false;

        if let Some(ref rx) = *ctx.mailbox_rx {
            while let Ok(msg) = rx.try_recv() {
                log::trace!("Mailbox message: from={} kind={}", msg.from, msg.kind);
                if msg.message == "/clear" {
                    let _ = ctx.worker_tx.send(WorkerCommand::ClearSession);
                    ctx.ui.clear_transcript();
                    handled = true;
                    continue;
                }
                let prompt = format_mailbox_prompt(&msg.from, &msg.kind, &msg.message);
                if !*ctx.worker_active {
                    ctx.ui.display_incoming_message(&prompt);
                    match ctx.worker_tx.send(WorkerCommand::ExecuteTurn {
                        prompt,
                        span: ctx.span,
                    }) {
                        Ok(()) => {
                            *ctx.worker_active = true;
                            handled = true;
                            break;
                        }
                        Err(_) => {
                            return StageOutcome::Fatal(LabeledError::new("Worker channel closed"));
                        }
                    }
                } else {
                    self.pending_mailbox_prompts.push(prompt);
                    log::trace!(
                        "Mailbox prompt queued: pending={}",
                        self.pending_mailbox_prompts.len()
                    );
                    handled = true;
                }
            }
        }

        // Drain pending mailbox prompts when worker becomes idle
        if !*ctx.worker_active && !self.pending_mailbox_prompts.is_empty() {
            let remaining = self.pending_mailbox_prompts.len() - 1;
            if let Some(prompt) = self.pending_mailbox_prompts.drain(0..1).next() {
                log::trace!("Draining mailbox prompt: remaining={remaining}");
                ctx.ui.display_incoming_message(&prompt);
                match ctx.worker_tx.send(WorkerCommand::ExecuteTurn {
                    prompt,
                    span: ctx.span,
                }) {
                    Ok(()) => {
                        *ctx.worker_active = true;
                        handled = true;
                    }
                    Err(_) => {
                        return StageOutcome::Fatal(LabeledError::new("Worker channel closed"));
                    }
                }
            }
        }

        if handled {
            StageOutcome::Handled
        } else {
            StageOutcome::Idle
        }
    }
}

fn format_mailbox_prompt(from: &str, kind: &str, message: &str) -> String {
    match kind {
        "task" => format!("[TASK from: {from}] {message}"),
        "completion" => format!("[COMPLETED from: {from}] {message}"),
        "question" => format!("[QUESTION from: {from} — BLOCKED, needs your decision] {message}"),
        _ => format!("[from: {from}] {message}"),
    }
}
