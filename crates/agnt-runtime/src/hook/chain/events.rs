//! The model-side hook events: the completion call, the streamed text deltas,
//! the finished turn, and a tool call rig refused.

use std::sync::Arc;

use rig::agent::{
    CompletionCallAction, CompletionCallEvent, HookContext, InvalidToolCallAction,
    InvalidToolCallContext, ModelTurnAction, ModelTurnFinished, ObservationAction, TextDelta,
};
use rig::message::Message;
use rig::wasm_compat::WasmCompatSend;

use agnt_kernel::{InferenceRecord, NoticeKind, SessionRecord, StopCause};

use crate::hook::output_repetition::{
    OUTPUT_REPETITION_NOTICE, OutputRepetitionDetector, RepetitionDetection,
};
use crate::hook::permission_resolver::AsyncPermissionResolver;
use agnt_kernel::bus::NarrationEvent;

use super::HookChain;
use super::compaction::{CompactionPath, decide_compaction};
use super::not_run::CANCELLED_STOP_REASON;

impl<P: AsyncPermissionResolver> HookChain<P> {
    /// Store the history and prompt, reset the sub-turn cap, and decide whether
    /// the completion should schedule a compaction.
    pub(super) fn begin_completion(
        &self,
        _ctx: &HookContext,
        event: CompletionCallEvent<'_>,
    ) -> impl std::future::Future<Output = CompletionCallAction> + WasmCompatSend {
        // 1. History snapshot — store history + prompt before the LLM call
        let history: Vec<Message> = event.history.iter().map(|m| (**m).clone()).collect();
        self.history.update(&history, event.prompt);

        // 2. Reset sub-turn cap — new LLM request, fresh counter
        self.subturn.reset();

        // Trace-level payload logging for debugging LLM request contents.
        if log::log_enabled!(log::Level::Trace) {
            log::trace!(
                "on_completion_call: history_len={} prompt={}",
                event.history.len(),
                serde_json::to_string(event.prompt).unwrap_or_else(|_| "<serialize error>".into()),
            );
            for (i, msg) in event.history.iter().enumerate() {
                log::trace!(
                    "  history[{}]: {}",
                    i,
                    serde_json::to_string(msg).unwrap_or_else(|_| "<serialize error>".into()),
                );
            }
        } else if log::log_enabled!(log::Level::Debug) {
            log::trace!("on_completion_call: history_len={}", event.history.len());
        }

        let cancelled = self.is_cancelled();
        let bus = self.bus.clone();
        let memory = Arc::clone(&self.memory);
        let conversation_id = self.conversation_id.clone();
        let compaction = self.compaction.clone();
        let last_total_tokens = Arc::clone(&self.last_total_tokens);
        let store_latch = Arc::clone(&self.store_latch);
        let stop = self.stop.clone();
        let prompt = event.prompt.clone();

        async move {
            if cancelled {
                stop.narrate(
                    bus.stream(),
                    StopCause::UserCancelled,
                    CANCELLED_STOP_REASON,
                );
                return CompletionCallAction::stop(CANCELLED_STOP_REASON);
            }
            let _ = bus.narration().publish(NarrationEvent::InferenceStarted);
            // Zero retention: a failed publish means no subscriber is attached
            // (nobody is listening), which is not a turn error.
            let _ = bus
                .stream()
                .publish(SessionRecord::Inference(InferenceRecord::Started));

            // Compaction decision: schedule a compaction when the session's
            // current conversation is over the threshold. The hook never patches
            // the per-turn history — the splice is what the next run loads.
            let path = CompactionPath::new(
                memory.as_ref(),
                &compaction,
                &last_total_tokens,
                &store_latch,
            );
            decide_compaction(
                &prompt,
                conversation_id.as_str(),
                &path,
                self.bus.control(),
                self.bus.stream(),
                self.bus.lifecycle(),
            )
            .await;

            CompletionCallAction::continue_run()
        }
    }

    /// Publish the streamed delta, stopping the stream when the assistant keeps
    /// repeating its own output.
    pub(super) fn stream_delta(
        &self,
        _ctx: &HookContext,
        event: TextDelta<'_>,
    ) -> impl std::future::Future<Output = ObservationAction> + WasmCompatSend {
        let cancelled = self.is_cancelled();
        let bus = self.bus.clone();
        let stop = self.stop.clone();
        let delta = event.delta.to_string();
        let aggregated = event.aggregated.to_string();
        // Intra-stream repetition detection: check the full aggregated stream
        // text (not individual deltas — streaming servers tokenize differently
        // each pass, so delta chunks are never reliably identical, but the
        // repetition is fully visible in the aggregated suffix). The 100-char
        // growth throttle inside `check_aggregated` bounds the check cost.
        // Compute it synchronously before the `async move` so `event` is not
        // borrowed across the await point. Skipped entirely when the repetition
        // guard is disabled.
        let repetition_action = if self.repetition_guard {
            OutputRepetitionDetector::check_aggregated(&self.output_repetition, event.aggregated)
        } else {
            None
        };

        async move {
            if cancelled {
                stop.narrate(
                    bus.stream(),
                    StopCause::UserCancelled,
                    CANCELLED_STOP_REASON,
                );
                return ObservationAction::stop(CANCELLED_STOP_REASON);
            }
            // Every intra-stream detection stops the stream: `check_aggregated`
            // returns Stop on ANY detection (the stream stops on the FIRST,
            // not after ladder escalations). NO warning here — the executor
            // surfaces the stop reason and drives the stop-to-steering retry
            // (append steering + reset ladder + re-run, terminal on the second
            // stop), the same path as the Esc-Esc cancel: `ObservationAction
            // ::stop` → Cancelled → cancel-reason matching →
            // `LlmEvent::Stopped` + `WarningEvent`.
            if repetition_action.is_some() {
                let reason = format!(
                    "{OUTPUT_REPETITION_NOTICE} the assistant kept repeating the same \
                     output after repeated steering. The run was stopped."
                );
                stop.narrate(bus.stream(), StopCause::OutputRepetition, &reason);
                return ObservationAction::stop(reason);
            }
            let _ = bus.narration().publish(NarrationEvent::InferenceChunk {
                delta: delta.clone(),
                aggregated: aggregated.clone(),
            });
            // Same deltas on the record stream: the hub event dies when the
            // surfaces fold records instead.
            let _ = bus
                .stream()
                .publish(SessionRecord::Inference(InferenceRecord::Chunk {
                    delta,
                    aggregated,
                }));
            ObservationAction::continue_run()
        }
    }

    /// Advance or stop the repetition ladder at the turn boundary.
    pub(super) fn check_turn_repetition(
        &self,
        _ctx: &HookContext,
        event: ModelTurnFinished<'_>,
    ) -> impl std::future::Future<Output = ModelTurnAction> + WasmCompatSend {
        // Detect assistant-output repetition at the turn boundary. The action
        // must be computable before the `async move` so it is decided
        // synchronously. `finish_turn` consumes the intra-stream detection
        // flag (escalating the ladder when intra-stream detection fired) and
        // otherwise falls back to the cross-turn full-text comparison.
        // Skipped entirely when the repetition guard is disabled.
        let repetition_detection = if self.repetition_guard {
            OutputRepetitionDetector::finish_turn(&self.output_repetition, event.content)
        } else {
            None
        };
        let bus = self.bus.clone();
        let stop = self.stop.clone();
        async move {
            if let Some(detection) = repetition_detection {
                match detection {
                    RepetitionDetection::First(message) | RepetitionDetection::Backoff(message) => {
                        let _ = bus.narration().publish(NarrationEvent::SystemMessage {
                            kind: NoticeKind::Warning,
                            text: message.clone(),
                        });
                        return ModelTurnAction::retry_with_feedback(message);
                    }
                    RepetitionDetection::Stop(message) => {
                        // No hook-side warning — the executor surfaces the stop
                        // reason, mirroring the tool doom loop. This is the
                        // repetition ladder's terminal stop, not the doom loop:
                        // the cause is what the steering retry keys on.
                        stop.narrate(bus.stream(), StopCause::OutputRepetition, &message);
                        return ModelTurnAction::stop(message);
                    }
                }
            }
            ModelTurnAction::continue_run()
        }
    }

    /// Warn on the unavailable tool and hand the model the retry feedback.
    pub(super) fn reject_invalid_tool_call(
        &self,
        _ctx: &HookContext,
        event: &InvalidToolCallContext,
    ) -> impl std::future::Future<Output = Option<InvalidToolCallAction>> + WasmCompatSend {
        log::warn!("Invalid tool call: tool={}", event.tool_name);
        let feedback = format!(
            "Tool '{}' is not available. Available tools: [{}]",
            event.tool_name,
            event.available_tools.join(", ")
        );
        let bus = self.bus.clone();
        async move {
            let _ = bus.narration().publish(NarrationEvent::SystemMessage {
                kind: NoticeKind::Warning,
                text: feedback.clone(),
            });
            Some(InvalidToolCallAction::retry(feedback))
        }
    }
}
