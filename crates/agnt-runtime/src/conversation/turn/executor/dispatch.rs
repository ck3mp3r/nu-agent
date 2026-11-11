//! Post-loop error dispatch for `TurnExecutor::execute`.
//!
//! Handles the terminal result of the retry/steering loop: the user-facing
//! error for an unknown tool or a hard failure, the doom/repetition stop
//! surfacing on cancellation, and the completed-turn tail (last_total_tokens,
//! doom reset, bus events, response_data capture).
//!
//! rig saves the conversation when a run succeeds and saves nothing when it
//! fails or is cancelled. Every non-success terminal arm therefore persists the
//! turn's delta through `persist_turn_delta`; the completed tail writes nothing.

use agnt_kernel::bus::{LifecycleEvent, NarrationEvent};
use agnt_kernel::{InferenceRecord, NoticeKind, SessionRecord, StopCause, TurnRecord};
use nu_protocol::{LabeledError, Span};

use crate::hook::chain::{NotRunCall, NotRunOutcome, NotRunStart, emit_not_run_dispatch};
use crate::hook::permission_resolver::resolve_tool_source;
use crate::hook::turn_tools::undispatched_streamed_calls;
use crate::types::Message;

use super::TurnRecovery;
use super::core::{
    ExecuteOutcome, NO_OUTPUT_STATEMENT, TURN_STOPPED_NOTICE, TurnExecutor, TurnResponseData,
};
use super::error_kind::{CompletionErrorKind, kind_to_user_msg};
use crate::conversation::turn::TurnError;
use crate::conversation::turn::TurnResult;
use crate::conversation::turn::{TurnBudget, TurnExhaustion, TurnFailure};

impl<'a, S> TurnExecutor<'a, S>
where
    S: crate::conversation::managers::SessionManager<
            Memory = crate::conversation::state::memory::MemoryOf,
            InnerMemory = crate::session::CachedMemory,
        >,
{
    /// Dispatch the terminal result of the retry/steering loop: surface stop
    /// reasons and produce the final `ExecuteOutcome` / `LabeledError`.
    pub(super) async fn dispatch_result(
        &mut self,
        visitor_result: Result<
            TurnResult,
            (
                TurnError,
                Box<crate::conversation::turn::error::TurnContext>,
            ),
        >,
        prompt: String,
        span: Span,
        final_session_id: Option<&str>,
        turn_started: std::time::Instant,
        task_id: Option<&str>,
    ) -> Result<ExecuteOutcome, Box<TurnFailure>> {
        let turn_result = match visitor_result {
            Ok(result) => result,
            Err((crate::conversation::turn::TurnError::UnknownTool { msg, messages, .. }, ctx)) => {
                self.persist_turn_delta(
                    final_session_id,
                    &prompt,
                    &messages,
                    ctx.pre_turn_message_count,
                    "[interrupted — turn stopped before responding]",
                    TurnRecovery {
                        tool_records: &ctx.tool_records,
                        streamed_text: None,
                    },
                )
                .await;
                log::warn!("Unknown tool call (retries exhausted): {msg}");
                let user_msg =
                    "Turn failed: The model attempted to call an unknown tool and could not \
                     recover. The session has been saved — try rephrasing your request."
                        .to_string();
                return Err(Box::new(TurnFailure::from_error(
                    LabeledError::new(user_msg.clone()).with_label(user_msg, span),
                )));
            }
            Err((e, ctx)) => {
                // Hard error: CompletionFailed, ToolExecutionFailed, or a turn the
                // tool-turn limit ended. rig saved nothing, so the turn's delta is
                // persisted here.

                // Extract the typed fields the user-facing message needs.
                let (msg, kind_opt, exhaustion) = match &e {
                    crate::conversation::turn::TurnError::CompletionFailed {
                        msg,
                        kind,
                        exhaustion,
                    } => (msg, Some(kind), *exhaustion),
                    crate::conversation::turn::TurnError::ToolExecutionFailed { msg } => {
                        (msg, None, None)
                    }
                    // Terminal MaxTurnsExceeded carries rig's full chat_history.
                    crate::conversation::turn::TurnError::MaxTurnsExceeded {
                        msg,
                        messages,
                        max_turns,
                    } => {
                        self.persist_turn_delta(
                            final_session_id,
                            &prompt,
                            messages,
                            ctx.pre_turn_message_count,
                            &format!("[Turn failed: {msg}]"),
                            TurnRecovery {
                                tool_records: &ctx.tool_records,
                                streamed_text: None,
                            },
                        )
                        .await;
                        let text = e.to_string();
                        return Err(Box::new(TurnFailure::with_budget(
                            LabeledError::new(text.clone()).with_label(text, span),
                            TurnBudget::MaxTurns { limit: *max_turns },
                        )));
                    }
                    other => {
                        let text = other.to_string();
                        return Err(Box::new(TurnFailure::from_error(
                            LabeledError::new(text.clone()).with_label(text, span),
                        )));
                    }
                };

                // The retry/steering loop records what it exhausted as data, so
                // the user-facing text is composed here from that data and the
                // pre-classified category — never parsed back out of a message.
                let user_msg = match (kind_opt, exhaustion) {
                    (_, Some(TurnExhaustion::Retries { attempts })) => {
                        format!("Turn failed after {attempts} retries. {msg}")
                    }
                    (Some(kind), Some(TurnExhaustion::NoOutput)) => {
                        format!("{} {NO_OUTPUT_STATEMENT}", kind_to_user_msg(kind, msg))
                    }
                    (Some(kind), None) => kind_to_user_msg(kind, msg),
                    // ToolExecutionFailed — no kind, use raw message
                    (None, _) => format!("Turn failed: {msg}"),
                };
                // The one stop a client names beyond a failure: the model's
                // output-token ceiling.
                let budget = match kind_opt {
                    Some(CompletionErrorKind::OutputBudget) => Some(TurnBudget::MaxTokens),
                    _ => None,
                };

                log::error!(
                    "Turn failed with unrecoverable error: session={:?} error={}",
                    final_session_id,
                    &msg[..msg.floor_char_boundary(200)]
                );
                self.persist_turn_delta(
                    final_session_id,
                    &prompt,
                    &ctx.last_known_history,
                    ctx.pre_turn_message_count,
                    &format!("[Turn failed: {msg}]"),
                    TurnRecovery {
                        tool_records: &ctx.tool_records,
                        streamed_text: None,
                    },
                )
                .await;
                return Err(Box::new(TurnFailure {
                    error: LabeledError::new(user_msg.clone()).with_label(user_msg, span),
                    budget,
                    exhaustion,
                }));
            }
        };

        // Cancelled: rig saved nothing, so the turn's delta is persisted here.
        // Surface the stop reason and return the early-exit response.
        if turn_result.cancelled {
            // The turn's wall-clock span, as both turn-end paths report it.
            let duration_ms = turn_started.elapsed().as_millis() as u64;
            // The hook-Terminate flavor carries rig's chat_history; the
            // bus-cancel flavor drops the stream and leaves only the hook's
            // snapshot of the last LLM call.
            let history: &[Message] = turn_result
                .messages
                .as_deref()
                .unwrap_or(turn_result.last_known_history.as_slice());
            self.persist_turn_delta(
                final_session_id,
                &prompt,
                history,
                turn_result.pre_turn_message_count,
                "[interrupted — turn stopped before responding]",
                TurnRecovery {
                    tool_records: &turn_result.tool_records,
                    streamed_text: Some(&turn_result.text),
                },
            )
            .await;
            // A stop drops the stream before the batch's later calls dispatch,
            // so nothing emitted their pair. Flush each one as a denied pair,
            // in commit order, so the live transcript renders it. The flush
            // records nothing: the persist reads records only, and rig's
            // closure rows already cover these calls in the log.
            for call in undispatched_streamed_calls(
                &turn_result.streamed_tool_calls,
                &turn_result.tool_records,
            ) {
                let name = call.function.name.as_str();
                let arguments = call.function.arguments_json();
                let source = resolve_tool_source(
                    name,
                    &self.tool_infra.closure_registry,
                    &self.tool_infra.mcp_registry,
                );
                emit_not_run_dispatch(
                    self.tool_infra.bus.stream(),
                    NotRunCall {
                        tool_name: name,
                        source,
                        arguments: &arguments,
                        start: NotRunStart::Publish,
                    },
                    NotRunOutcome::Cancelled,
                )
                .await;
            }
            // Surface the stop reason if this cancellation was a doom or
            // repetition stop. Fix 1: the warning is sent BEFORE
            // `TurnRecord::Completed` so the TUI reduces the warning ahead of
            // turn finalize (the race with the separate bus channels is
            // bounded; the TUI-side finalize fix preserves warnings
            // deterministically). Fix 2: the reason rides `LlmEvent::Stopped`,
            // never `AssistantMessage` — routing it through the
            // assistant-stream dedup path would truncate the streamed repeated
            // block. A user cancel carries no stop reason, so it gets the
            // generic notice instead: the turn's completed work is kept, and
            // without the notice the user sees no sign the turn stopped.
            let response_text =
                match (&turn_result.cancel_reason, turn_result.stop_cause) {
                    (
                        Some(reason),
                        Some(cause @ (StopCause::DoomLoop | StopCause::OutputRepetition)),
                    ) => {
                        let _ = self.tool_infra.bus.narration().publish(
                            NarrationEvent::SystemMessage {
                                kind: NoticeKind::Warning,
                                text: reason.clone(),
                            },
                        );
                        let _ = self.tool_infra.bus.narration().publish(
                            NarrationEvent::InferenceStopped {
                                reason: reason.clone(),
                            },
                        );
                        let _ = self
                            .tool_infra
                            .bus
                            .stream()
                            .publish(SessionRecord::Inference(InferenceRecord::Stopped {
                                cause,
                                reason: reason.clone(),
                            }));
                        reason.clone()
                    }
                    _ => {
                        let _ = self.tool_infra.bus.narration().publish(
                            NarrationEvent::InferenceStopped {
                                reason: TURN_STOPPED_NOTICE.to_string(),
                            },
                        );
                        let _ = self
                            .tool_infra
                            .bus
                            .stream()
                            .publish(SessionRecord::Inference(InferenceRecord::Stopped {
                                cause: turn_result.stop_cause.unwrap_or(StopCause::UserCancelled),
                                reason: TURN_STOPPED_NOTICE.to_string(),
                            }));
                        String::new()
                    }
                };
            // The turn's one unfoldable fact — every other turn figure folds
            // from the records this stream already carried.
            let _ = self
                .tool_infra
                .bus
                .stream()
                .publish(SessionRecord::Turn(TurnRecord::Completed { duration_ms }));
            let _ = self
                .tool_infra
                .bus
                .lifecycle()
                .publish(LifecycleEvent::TurnCompleted {
                    task_id: task_id.map(str::to_string),
                    duration_ms,
                });
            return Ok(ExecuteOutcome::EarlyReturn(crate::llm::format_response(
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
        // rig saved the conversation when the run succeeded, and the turn's own
        // settle recorded its facts, so nothing is written here.
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
        self.tool_infra.lock_doom_state().reset();

        // Emit the tail delta on the narration and record concerns
        if !turn_result.deltas_emitted {
            let full_text = turn_result.text.clone();
            let _ = self
                .tool_infra
                .bus
                .narration()
                .publish(NarrationEvent::InferenceChunk {
                    delta: full_text.clone(),
                    aggregated: full_text.clone(),
                });
            let _ = self
                .tool_infra
                .bus
                .stream()
                .publish(SessionRecord::Inference(InferenceRecord::Chunk {
                    delta: full_text.clone(),
                    aggregated: full_text,
                }));
        }
        // The turn's one unfoldable fact — every other turn figure folds from
        // the records this stream already carried.
        let duration_ms = turn_started.elapsed().as_millis() as u64;
        let _ = self
            .tool_infra
            .bus
            .stream()
            .publish(SessionRecord::Turn(TurnRecord::Completed { duration_ms }));
        let _ = self
            .tool_infra
            .bus
            .lifecycle()
            .publish(LifecycleEvent::TurnCompleted {
                task_id: task_id.map(str::to_string),
                duration_ms,
            });

        // Store turn data for response building
        self.response_data = Some(TurnResponseData {
            text: turn_result.text,
            usage: turn_result.usage,
            has_session: final_session_id.is_some(),
        });

        Ok(ExecuteOutcome::Completed)
    }
}
