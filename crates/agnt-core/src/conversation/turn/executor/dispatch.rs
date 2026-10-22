//! Post-loop error dispatch for `TurnExecutor::execute`.
//!
//! Handles the terminal result of the retry/steering loop: the user-facing
//! error for an unknown tool or a hard failure, the doom/repetition stop
//! surfacing on cancellation, and the completed-turn tail (last_total_tokens,
//! doom reset, bus events, response_data capture).
//!
//! Nothing is persisted here: rig saves the conversation when a run succeeds
//! and saves nothing when it fails or is cancelled.

use nu_protocol::{LabeledError, Span};

use crate::hook::doom_loop::DOOM_LOOP_STOP_PREFIX;
use crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX;
use crate::protocol::event::UiEvent;

use super::core::{NO_OUTPUT_STATEMENT, TurnExecutor, TurnOutcome, TurnResponseData};
use super::error_kind::kind_to_user_msg;
use crate::conversation::turn::TurnError;
use crate::conversation::turn::TurnResult;

impl<'a, S> TurnExecutor<'a, S>
where
    S: crate::conversation::managers::SessionManager<
            Memory = crate::conversation::state::memory::MemoryOf,
            InnerMemory = crate::session::CachedMemory,
        >,
{
    /// Dispatch the terminal result of the retry/steering loop: surface stop
    /// reasons and produce the final `TurnOutcome` / `LabeledError`.
    pub(super) async fn dispatch_result(
        &mut self,
        visitor_result: Result<
            TurnResult,
            (TurnError, crate::conversation::turn::error::TurnContext),
        >,
        span: Span,
        final_session_id: Option<&str>,
    ) -> Result<TurnOutcome, LabeledError> {
        let turn_result = match visitor_result {
            Ok(result) => result,
            Err((crate::conversation::turn::TurnError::UnknownTool { msg, .. }, _)) => {
                log::warn!("Unknown tool call (retries exhausted): {msg}");
                let user_msg =
                    "Turn failed: The model attempted to call an unknown tool and could not \
                     recover. Try rephrasing your request."
                        .to_string();
                return Err(LabeledError::new(user_msg.clone()).with_label(user_msg, span));
            }
            Err((e, _)) => {
                // Hard error: CompletionFailed or ToolExecutionFailed. Nothing is
                // persisted: rig saves a conversation only when a run succeeds.

                // Extract the fields the user-facing message needs.
                let (msg, kind_opt) = match &e {
                    crate::conversation::turn::TurnError::CompletionFailed { msg, kind } => {
                        (msg, Some(kind))
                    }
                    crate::conversation::turn::TurnError::ToolExecutionFailed { msg } => {
                        (msg, None)
                    }
                    _ => {
                        return Err(
                            LabeledError::new(e.to_string()).with_label(e.to_string(), span)
                        );
                    }
                };

                // For retry-exhausted errors, the message already contains the retry count.
                // Otherwise, use the pre-classified kind to build the user-facing message.
                let user_msg = if msg.starts_with("Turn failed after") {
                    msg.clone()
                } else if let Some(kind) = kind_opt {
                    let base = kind_to_user_msg(kind, msg);
                    // Preserve the explicit no-output statement appended when
                    // steering is exhausted (criterion 3).
                    if msg.ends_with(NO_OUTPUT_STATEMENT) {
                        format!("{base} {NO_OUTPUT_STATEMENT}")
                    } else {
                        base
                    }
                } else {
                    // ToolExecutionFailed — no kind, use raw message
                    format!("Turn failed: {msg}")
                };

                log::error!(
                    "Turn failed with unrecoverable error: session={:?} error={}",
                    final_session_id,
                    &msg[..msg.floor_char_boundary(200)]
                );
                return Err(LabeledError::new(user_msg.clone()).with_label(user_msg, span));
            }
        };

        // Cancelled: rig saved nothing, so nothing is persisted here. Surface the
        // stop reason and return the early-exit response.
        if turn_result.cancelled {
            // Surface the doom-loop stop reason if this cancellation was a doom
            // stop. Fix 1: the warning is sent BEFORE `TurnEvent::Completed` so
            // the TUI reduces the warning ahead of turn finalize (the race with
            // the separate bus channels is bounded; the TUI-side finalize fix
            // preserves warnings deterministically). Fix 2: the reason rides
            // `LlmEvent::Stopped`, never `AssistantMessage` — routing it
            // through the assistant-stream dedup path would truncate the
            // streamed repeated block.
            let response_text = match &turn_result.cancel_reason {
                Some(reason)
                    if reason.starts_with(DOOM_LOOP_STOP_PREFIX)
                        || reason.starts_with(OUTPUT_REPETITION_STOP_PREFIX) =>
                {
                    let _ = self
                        .tool_infra
                        .bus
                        .ui_event()
                        .send(UiEvent::Warning {
                            message: reason.clone(),
                        })
                        .await;
                    let _ = self
                        .tool_infra
                        .bus
                        .ui_event()
                        .send(UiEvent::Stopped {
                            reason: reason.clone(),
                        })
                        .await;
                    reason.clone()
                }
                _ => String::new(),
            };
            let _ = self
                .tool_infra
                .bus
                .ui_event()
                .send(UiEvent::Completed {
                    tool_calls: turn_result.tool_call_count,
                })
                .await;
            return Ok(TurnOutcome::EarlyReturn(crate::llm::format_response(
                &crate::llm::LlmResponse {
                    text: response_text,
                    usage: crate::llm::LlmUsage {
                        input_tokens: 0,
                        output_tokens: 0,
                        total_tokens: 0,
                        cached_input_tokens: 0,
                        cache_creation_input_tokens: 0,
                    },
                    tool_calls: Vec::new(),
                    tool_call_metadata: Vec::new(),
                },
                self.config,
                final_session_id,
                span,
            )));
        }

        // Completed turn: update last_total_tokens for compaction tracking.
        // rig saved the conversation when the run succeeded, so nothing is
        // persisted here.
        if final_session_id.is_some() {
            *self.memory_state.last_total_tokens_mut() = Some(turn_result.last_total_tokens);
            log::debug!(
                "Turn completed: session={:?} last_total_tokens={}",
                final_session_id,
                turn_result.last_total_tokens
            );
        }

        // Successful turn: reset doom loop state so accumulated signatures
        // from prior turns don't carry over into the next healthy turn.
        self.tool_infra.doom_state.lock().unwrap().reset();

        // Emit UI events via bus
        if !turn_result.deltas_emitted {
            let _ = self
                .tool_infra
                .bus
                .ui_event()
                .send(UiEvent::AssistantMessage {
                    delta: turn_result.text.clone(),
                    text: turn_result.text.clone(),
                })
                .await;
        }
        let _ = self
            .tool_infra
            .bus
            .ui_event()
            .send(UiEvent::Completed {
                tool_calls: turn_result.tool_call_count,
            })
            .await;

        // Store turn data for response building
        self.response_data = Some(TurnResponseData {
            text: turn_result.text,
            usage: turn_result.usage,
            has_session: final_session_id.is_some(),
        });

        Ok(TurnOutcome::Completed)
    }
}
