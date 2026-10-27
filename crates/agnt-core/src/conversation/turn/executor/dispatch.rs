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

use nu_protocol::{LabeledError, Span};

use crate::bus::TurnEvent;
use crate::hook::chain::{NotRunCall, NotRunOutcome, NotRunStart, emit_not_run_dispatch};
use crate::hook::doom_loop::DOOM_LOOP_STOP_PREFIX;
use crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX;
use crate::hook::permission_resolver::resolve_tool_source;
use crate::hook::turn_tools::undispatched_streamed_calls;
use crate::protocol::event::UiEvent;
use crate::types::Message;

use super::TurnRecovery;
use super::core::{
    NO_OUTPUT_STATEMENT, TURN_STOPPED_NOTICE, TurnExecutor, TurnOutcome, TurnResponseData,
};
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
            (
                TurnError,
                Box<crate::conversation::turn::error::TurnContext>,
            ),
        >,
        prompt: String,
        span: Span,
        final_session_id: Option<&str>,
        turn_started: std::time::Instant,
    ) -> Result<TurnOutcome, LabeledError> {
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
                return Err(LabeledError::new(user_msg.clone()).with_label(user_msg, span));
            }
            Err((e, ctx)) => {
                // Hard error: CompletionFailed or ToolExecutionFailed. rig saved
                // nothing, so the turn's delta is persisted here.

                // Extract the fields the user-facing message needs.
                let (msg, kind_opt) = match &e {
                    crate::conversation::turn::TurnError::CompletionFailed { msg, kind } => {
                        (msg, Some(kind))
                    }
                    crate::conversation::turn::TurnError::ToolExecutionFailed { msg } => {
                        (msg, None)
                    }
                    // Terminal MaxTurnsExceeded carries rig's full chat_history.
                    crate::conversation::turn::TurnError::MaxTurnsExceeded {
                        msg,
                        messages,
                        ..
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
                        return Err(
                            LabeledError::new(e.to_string()).with_label(e.to_string(), span)
                        );
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
                return Err(LabeledError::new(user_msg.clone()).with_label(user_msg, span));
            }
        };

        // Cancelled: rig saved nothing, so the turn's delta is persisted here.
        // Surface the stop reason and return the early-exit response.
        if turn_result.cancelled {
            // The turn's usage and wall-clock span, as both turn-end paths
            // report them.
            let usage = self.tool_infra.turn_usage.totals();
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
                )
                .as_str();
                let call_line = self.tool_infra.render_registry.render(name, &arguments);
                emit_not_run_dispatch(
                    &self.tool_infra.bus,
                    &self.tool_infra.cwd,
                    NotRunCall {
                        tool_name: name,
                        source,
                        arguments: &arguments,
                        start: NotRunStart::Publish(call_line),
                    },
                    NotRunOutcome::Cancelled,
                    true,
                )
                .await;
            }
            // Surface the stop reason if this cancellation was a doom or
            // repetition stop. Fix 1: the warning is sent BEFORE
            // `TurnEvent::Completed` so the TUI reduces the warning ahead of
            // turn finalize (the race with the separate bus channels is
            // bounded; the TUI-side finalize fix preserves warnings
            // deterministically). Fix 2: the reason rides `LlmEvent::Stopped`,
            // never `AssistantMessage` — routing it through the
            // assistant-stream dedup path would truncate the streamed repeated
            // block. A user cancel carries no stop reason, so it gets the
            // generic notice instead: the turn's completed work is kept, and
            // without the notice the user sees no sign the turn stopped.
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
                _ => {
                    let _ = self
                        .tool_infra
                        .bus
                        .ui_event()
                        .send(UiEvent::Stopped {
                            reason: TURN_STOPPED_NOTICE.to_string(),
                        })
                        .await;
                    String::new()
                }
            };
            let _ = self
                .tool_infra
                .bus
                .ui_event()
                .send(UiEvent::Completed {
                    tool_calls: turn_result.tool_call_count,
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.total_tokens,
                    cost: usage.cost,
                    cost_complete: usage.cost_complete,
                    duration_ms,
                })
                .await;
            let _ = self
                .tool_infra
                .bus
                .turn()
                .send(TurnEvent::Completed {
                    tool_calls: turn_result.tool_call_count,
                    input_tokens: usage.input_tokens,
                    output_tokens: usage.output_tokens,
                    total_tokens: usage.total_tokens,
                    cost: usage.cost,
                    cost_complete: usage.cost_complete,
                    duration_ms,
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
        // The turn's usage and wall-clock span, as both turn-end paths report
        // them.
        let usage = self.tool_infra.turn_usage.totals();
        let duration_ms = turn_started.elapsed().as_millis() as u64;
        let _ = self
            .tool_infra
            .bus
            .ui_event()
            .send(UiEvent::Completed {
                tool_calls: turn_result.tool_call_count,
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
                cost: usage.cost,
                cost_complete: usage.cost_complete,
                duration_ms,
            })
            .await;
        let _ = self
            .tool_infra
            .bus
            .turn()
            .send(TurnEvent::Completed {
                tool_calls: turn_result.tool_call_count,
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
                cost: usage.cost,
                cost_complete: usage.cost_complete,
                duration_ms,
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
