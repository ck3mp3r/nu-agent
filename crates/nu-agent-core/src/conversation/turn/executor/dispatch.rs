//! Post-loop error dispatch for `TurnExecutor::execute`.
//!
//! Handles the terminal result of the retry/steering loop: UnknownTool history
//! recovery, Path A/C/B cancelled persistence + doom/repetition stop surfacing,
//! hard-error persistence with placeholder synthesis, and the completed-turn
//! tail (last_total_tokens, doom reset, bus events, response_data capture).

use nu_protocol::{LabeledError, Span};

use crate::bus::{LlmEvent, TurnEvent, WarningEvent};
use crate::hook::doom_loop::DOOM_LOOP_STOP_PREFIX;
use crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX;
use crate::session::repair::inject_missing_tool_results;
use crate::types::Message;

use super::core::{NO_OUTPUT_STATEMENT, TurnExecutor, TurnOutcome, TurnResponseData};
use super::error_kind::kind_to_user_msg;
use super::response::close_open_tool_result_block;
use crate::conversation::turn::TurnError;
use crate::conversation::turn::TurnResult;

impl<'a, ST, S> TurnExecutor<'a, S, ST>
where
    ST: crate::session::SessionStore + Clone + Send + Sync + 'static,
    S: crate::conversation::managers::SessionManager<
            Memory = crate::conversation::state::memory::MemoryOf<ST>,
            InnerMemory = crate::session::CachedMemory<ST>,
        >,
{
    /// Dispatch the terminal result of the retry/steering loop: persist
    /// recovered history, surface stop reasons, and produce the final
    /// `TurnOutcome` / `LabeledError`.
    pub(super) async fn dispatch_result(
        &mut self,
        visitor_result: Result<
            TurnResult,
            (TurnError, crate::conversation::turn::error::TurnContext),
        >,
        prompt: String,
        span: Span,
        final_session_id: Option<&str>,
    ) -> Result<TurnOutcome, LabeledError> {
        let turn_result = match visitor_result {
            Ok(result) => result,
            Err((crate::conversation::turn::TurnError::UnknownTool { msg, messages, .. }, ctx)) => {
                if let Some(session_id) = final_session_id {
                    let delta: Vec<Message> = messages
                        .iter()
                        .skip(ctx.pre_turn_message_count)
                        .cloned()
                        .collect();
                    log::debug!(
                        "Path A non-cancelled (unknown tool): delta_count={} pre_turn={}",
                        delta.len(),
                        ctx.pre_turn_message_count
                    );
                    if !delta.is_empty() {
                        let patched = inject_missing_tool_results(delta);
                        self.append_to_memory_or_warn(
                            session_id,
                            patched,
                            "recovered history for unknown tool turn",
                        )
                        .await;
                    }
                }
                log::warn!("Unknown tool call (retries exhausted): {msg}");
                let user_msg =
                    "Turn failed: The model attempted to call an unknown tool and could not recover. \
                         The session has been saved — try rephrasing your request."
                        .to_string();
                return Err(LabeledError::new(user_msg.clone()).with_label(user_msg, span));
            }
            Err((crate::conversation::turn::TurnError::Cancelled { msg, messages }, ctx)) => {
                // Path A: rig hook cancelled — persist chat_history delta if available.
                // messages for PromptCancelled is rig's full_history() — it DOES include
                // the current-turn user prompt and any tool exchanges. skip(pre_turn_message_count)
                // yields only the new messages from this turn, preventing double-appending the
                // prior session history when a cancelled turn follows prior session messages.
                if let Some(session_id) = final_session_id {
                    let delta: Vec<Message> = messages
                        .iter()
                        .skip(ctx.pre_turn_message_count)
                        .cloned()
                        .collect();
                    log::debug!(
                        "Path A cancelled: delta_count={} pre_turn={}",
                        delta.len(),
                        ctx.pre_turn_message_count
                    );
                    if !delta.is_empty() {
                        let patched = inject_missing_tool_results(delta);
                        let patched = close_open_tool_result_block(patched, "[cancelled]");
                        self.append_to_memory_or_warn(
                            session_id,
                            patched,
                            "recovered history for cancelled turn (path A)",
                        )
                        .await;
                    }
                }
                // Stop-to-steering exhaustion: a repetition stop at the
                // per-turn retry cap falls through to this block, which
                // surfaces the terminal stop (mirroring the doom-loop stop
                // surface).
                // Surface the doom-loop stop reason if this cancellation was a doom stop.
                // Fix 2: the reason rides `LlmEvent::Stopped`, never
                // `AssistantMessage` — routing it through the assistant-stream
                // dedup path would truncate the streamed repeated block.
                let response_text = if msg.starts_with(DOOM_LOOP_STOP_PREFIX)
                    || msg.starts_with(OUTPUT_REPETITION_STOP_PREFIX)
                {
                    let _ = self
                        .tool_infra
                        .bus
                        .warning()
                        .send(WarningEvent::Message {
                            message: msg.clone(),
                        })
                        .await;
                    let _ = self
                        .tool_infra
                        .bus
                        .llm()
                        .send(LlmEvent::Stopped {
                            reason: msg.clone(),
                        })
                        .await;
                    msg
                } else {
                    String::new()
                };
                // Return a minimal cancelled response (not an error)
                let llm_response = crate::llm::LlmResponse {
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
                };
                return Ok(TurnOutcome::EarlyReturn(crate::llm::format_response(
                    &llm_response,
                    self.config,
                    final_session_id,
                    span,
                )));
            }
            Err((e, ctx)) => {
                // Hard error: CompletionFailed or ToolExecutionFailed.
                // If on_completion_call fired at least once, we have a partial history snapshot
                // from the hook Arc. Persist it so completed sub-turns are not lost.
                // If no on_completion_call fired (failure before first HTTP request), fall back
                // to a synthetic user+assistant placeholder pair to maintain alternating structure.

                // Extract fields needed for history persistence and error message.
                let (msg, kind_opt) = match &e {
                    crate::conversation::turn::TurnError::CompletionFailed { msg, kind } => {
                        (msg, Some(kind))
                    }
                    crate::conversation::turn::TurnError::ToolExecutionFailed { msg } => {
                        (msg, None)
                    }
                    // Other variants (Cancelled, MaxTurnsExceeded, UnknownTool) are already
                    // handled above; this catch-all is for exhaustiveness.
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
                if let Some(session_id) = final_session_id {
                    let delta: Vec<Message> = ctx
                        .last_known_history
                        .iter()
                        .skip(ctx.pre_turn_message_count)
                        .cloned()
                        .collect();
                    log::debug!(
                        "Hard error: delta_count={} pre_turn={}",
                        delta.len(),
                        ctx.pre_turn_message_count
                    );
                    if !delta.is_empty() {
                        let patched = inject_missing_tool_results(delta);
                        let patched = close_open_tool_result_block(patched, msg);
                        self.append_to_memory_or_warn(
                            session_id,
                            patched,
                            "recovered history on hard error",
                        )
                        .await;
                    } else {
                        // delta is empty: hook never fired OR error before any new messages
                        // were added. Synthesise a placeholder to maintain the alternating
                        // user/assistant structure.
                        log::debug!("Hard error: delta empty, synthesizing fallback placeholder");
                        let fallback = vec![
                            Message::user(prompt.clone()),
                            Message::assistant(format!("[Turn failed: {msg}]")),
                        ];
                        self.append_to_memory_or_warn(
                            session_id,
                            fallback,
                            "error placeholder on hard error",
                        )
                        .await;
                    }
                }
                return Err(LabeledError::new(user_msg.clone()).with_label(user_msg, span));
            }
        };

        // Path C: PromptCancelled was caught inside build_agent_and_stream and returned
        // as Ok(cancelled=true). Treat identically to Path A.
        //
        // Path B (defensive fallback) is folded into Path C: Ok(cancelled=true, messages=None).
        // Under rig v0.39+ semantics this branch is not expected to trigger — PromptCancelled
        // always carries chat_history. Kept as a safety net for future rig version changes.
        if turn_result.cancelled {
            if let Some(session_id) = final_session_id {
                if let Some(messages) = turn_result.messages {
                    // Normal path: rig provided chat_history via PromptCancelled.
                    // PromptCancelled.chat_history is rig's full_history() — it DOES include
                    // the current-turn user prompt and any tool exchanges. skip(pre_turn_message_count)
                    // yields only the new messages from this turn, preventing double-appending the
                    // prior session history when a cancelled turn follows prior session messages.
                    // JournalConversationMemory.append() writes both JSONL and in-memory
                    // cache in one call — no separate conversation_store.append() needed.
                    let delta: Vec<Message> = messages
                        .iter()
                        .skip(turn_result.pre_turn_message_count)
                        .cloned()
                        .collect();
                    log::debug!(
                        "Path C cancelled: delta_count={} pre_turn={}",
                        delta.len(),
                        turn_result.pre_turn_message_count
                    );
                    if !delta.is_empty() {
                        let patched = inject_missing_tool_results(delta);
                        let patched = close_open_tool_result_block(patched, "[cancelled]");
                        self.append_to_memory_or_warn(
                            session_id,
                            patched,
                            "recovered history for cancelled turn (path C)",
                        )
                        .await;
                    }
                } else {
                    // Path B: tokio::select cancelled before rig yielded PromptCancelled.
                    // Use last_known_history from the hook's snapshot of completed work
                    // (including tool calls and their results) instead of synthesizing a
                    // minimal placeholder that would lose all completed work from this turn.
                    let lkh = &turn_result.last_known_history;
                    if !lkh.is_empty() {
                        let delta: Vec<Message> = lkh
                            .iter()
                            .skip(turn_result.pre_turn_message_count)
                            .cloned()
                            .collect();
                        log::debug!(
                            "Path B fallback: last_known_history delta_count={}",
                            delta.len()
                        );
                        if !delta.is_empty() {
                            let patched = inject_missing_tool_results(delta);
                            let patched = close_open_tool_result_block(patched, "[cancelled]");
                            self.append_to_memory_or_warn(
                                session_id,
                                patched,
                                "recovered history for cancel path B (last known history)",
                            )
                            .await;
                        }
                    } else {
                        // True fallback: hook never fired, synthesize minimal placeholder
                        log::debug!("Path B true fallback: synthesizing minimal placeholder");
                        let mut cancelled_messages = vec![Message::user(prompt.clone())];
                        if !turn_result.text.is_empty() {
                            cancelled_messages.push(Message::assistant(turn_result.text.clone()));
                        }
                        self.append_to_memory_or_warn(
                            session_id,
                            cancelled_messages,
                            "cancel path B fallback",
                        )
                        .await;
                    }
                }
            }
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
                        .warning()
                        .send(WarningEvent::Message {
                            message: reason.clone(),
                        })
                        .await;
                    let _ = self
                        .tool_infra
                        .bus
                        .llm()
                        .send(LlmEvent::Stopped {
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
                .turn()
                .send(TurnEvent::Completed {
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
        // The actual JSONL write was already done by rig calling memory.append() at turn end.
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
                .llm()
                .send(LlmEvent::AssistantMessage {
                    text: turn_result.text.clone(),
                })
                .await;
        }
        let _ = self
            .tool_infra
            .bus
            .turn()
            .send(TurnEvent::Completed {
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
