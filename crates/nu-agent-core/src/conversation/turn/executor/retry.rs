//! Retry/steering loop for `TurnExecutor::execute`.
//!
//! The loop runs the turn future repeatedly, applying four independent retry
//! budgets (backoff `attempt`, `feedback_retries`, `max_turns_feedback_retries`,
//! `rep_stop_retries`), the OutputBudget max_tokens auto-raise, and the
//! session-less steering carrier. It returns the final `TurnResult` or the
//! terminal `(TurnError, TurnContext)` for post-loop dispatch.

use std::sync::Arc;

use crate::config::{Config, defaults};
use crate::conversation::turn::feedback::{
    FeedbackPhase, build_feedback_message, build_max_turns_feedback_message, is_model_correctable,
};
use crate::conversation::turn::{TurnContext, TurnConversation, TurnInput, error, execute_turn};
use crate::hook::output_repetition::{
    OUTPUT_REPETITION_BACKOFF_MESSAGE, OUTPUT_REPETITION_STOP_PREFIX,
};
use crate::hook::permission_resolver::AsyncPermissionResolver;
use crate::session::repair::inject_missing_tool_results;
use crate::types::Message;

use super::core::TurnExecutor;
use super::error_kind::CompletionErrorKind;
use super::response::extract_retry_after_ms;
use crate::bus::WarningEvent;
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
    /// Run the retry/steering loop. Returns the final `TurnResult` (Ok) or the
    /// terminal `(TurnError, TurnContext)` (Err) for post-loop dispatch.
    pub(super) async fn run_retry_loop<P: AsyncPermissionResolver>(
        &mut self,
        prompt: String,
        preamble: Option<String>,
        conversation_id: String,
        saved_tool_definitions: Vec<crate::types::ToolDefinition>,
        final_session_id: Option<&str>,
        permission_resolver: P,
    ) -> Result<TurnResult, (TurnError, error::TurnContext)> {
        let mut attempt = 0u8;
        // Per-turn feedback budget for model-correctable provider failures,
        // independent of the backoff `attempt` counter: feedback retries never
        // consume the backoff budget and vice versa.
        let mut feedback_retries = 0u8;
        // Per-turn steering budget for MaxTurnsExceeded failures, independent
        // of the backoff `attempt` and provider `feedback_retries` counters:
        // each steering retry appends one user-role message and re-runs the
        // turn with a fresh tool-call budget.
        let mut max_turns_feedback_retries = 0u8;
        // Per-turn steering budget for output-repetition guard stops:
        // converting a repetition stop into a steering retry (steering
        // message + ladder reset) re-runs the turn, up to the
        // MAX_REPETITION_STOP_RETRIES cap; a stop at the cap is terminal.
        // Independent of the other retry counters.
        let mut rep_stop_retries = 0u8;
        // Session-less steering carrier: with no session (final_session_id
        // None) the steering message cannot be appended to session memory, so
        // the conversion path prepends it to the retry prompt instead
        // (transient in-memory delivery; nothing is persisted).
        let mut session_less_steering: Option<String> = None;
        // Per-attempt Config override for the OutputBudget max_tokens auto-raise.
        // When set, this clone (with a raised max_tokens) is used for the next
        // attempt instead of `self.config`. Cleared implicitly by being replaced
        // each time a raise is applied; None means use the base config.
        let mut attempt_config: Option<Config> = None;
        loop {
            // Restore tool definitions on retry attempts (backoff, feedback,
            // or steering: the previous attempt consumed them via
            // std::mem::take)
            if attempt > 0
                || feedback_retries > 0
                || max_turns_feedback_retries > 0
                || rep_stop_retries > 0
            {
                self.tool_infra.visible_tool_definitions = saved_tool_definitions.clone();
            }

            // Reset doom loop state at the start of every turn attempt
            self.tool_infra
                .doom_state
                .lock()
                .expect("doom loop mutex poisoned")
                .reset();
            let attempt_cfg = attempt_config.as_ref();
            let turn_ctx = TurnContext::new(
                TurnConversation {
                    memory: Arc::clone(self.memory_state.memory()),
                    conversation_id: conversation_id.clone(),
                    has_session: final_session_id.is_some(),
                    shared_model: self.shared_model.clone(),
                    compaction: self.compaction.clone(),
                },
                TurnInput {
                    prompt: match &session_less_steering {
                        Some(steering) => format!("{steering}\n\n{prompt}"),
                        None => prompt.clone(),
                    },
                    preamble: preamble.as_deref(),
                    max_turns: self.config.max_tool_turns,
                },
                super::core::ToolInfra {
                    closure_registry: self.tool_infra.closure_registry.clone(),
                    mcp_registry: self.tool_infra.mcp_registry.clone(),
                    tool_server_handle: self.tool_infra.tool_server_handle.clone(),
                    visible_tool_definitions: std::mem::take(
                        &mut self.tool_infra.visible_tool_definitions,
                    ),
                    circuit_breaker: self.tool_infra.circuit_breaker.clone(),
                    doom_state: self.tool_infra.doom_state.clone(),
                    output_repetition: self.tool_infra.output_repetition.clone(),
                    repetition_guard: self.tool_infra.repetition_guard,
                    last_total_tokens: self.tool_infra.last_total_tokens.clone(),
                    bus: self.tool_infra.bus.clone(),
                },
                attempt_cfg.unwrap_or(self.config),
            );

            // Retries and the initial attempt both run the same turn future.
            let result = execute_turn(turn_ctx, permission_resolver.clone()).await;

            match &result {
                Err((
                    crate::conversation::turn::TurnError::CompletionFailed { kind, msg },
                    ctx,
                )) => {
                    // Hard error path — check if retryable using the pre-classified kind
                    let has_partial_history = !ctx.last_known_history.is_empty();
                    log::debug!(
                        "Hard error: kind={kind:?} retryable={} history_len={}",
                        kind.is_retryable(),
                        ctx.last_known_history.len()
                    );
                    // Feedback retry: model-correctable provider failures get
                    // one re-run per feedback message, capped per turn by
                    // MAX_PROVIDER_FEEDBACK_RETRIES. The kinds are disjoint
                    // from the backoff branch's retryable kinds, so the two
                    // budgets never interact.
                    if is_model_correctable(kind)
                        && feedback_retries < defaults::MAX_PROVIDER_FEEDBACK_RETRIES
                        && has_partial_history
                        && let Some(session_id) = final_session_id
                    {
                        feedback_retries += 1;
                        // Escalating steering: the first feedback is a gentle
                        // remedy, subsequent steers escalate to a stronger
                        // remedy so the model sees different guidance on each
                        // repeated failure.
                        let phase = if feedback_retries == 1 {
                            FeedbackPhase::First
                        } else {
                            FeedbackPhase::Backoff
                        };
                        let feedback = build_feedback_message(
                            kind,
                            phase,
                            msg,
                            self.config.output_budget_empty_remedy.as_deref(),
                            self.config.output_budget_remedy_mode.as_deref(),
                        );
                        log::warn!(
                            "Model-correctable error ({kind:?}), feedback retry {feedback_retries}/{}.",
                            defaults::MAX_PROVIDER_FEEDBACK_RETRIES
                        );
                        // Opt-in max_tokens auto-raise for OutputBudget retries:
                        // compute the raised value for the NEXT attempt only and
                        // stash it in `attempt_config`. The raise applies only
                        // when enabled, the kind is OutputBudget, the effective
                        // max_tokens is set, and the base is below the cap. The
                        // base is read from the attempt's effective config (the
                        // raised value from a prior retry), so consecutive
                        // OutputBudget failures compound the raise.
                        let effective = attempt_config.as_ref().unwrap_or(self.config);
                        if *kind == CompletionErrorKind::OutputBudget
                            && self
                                .config
                                .output_budget_raise_enabled
                                .unwrap_or(defaults::OUTPUT_BUDGET_RAISE_ENABLED)
                            && let Some(base) = effective.max_tokens.or(effective.max_output_tokens)
                            && base
                                < self
                                    .config
                                    .output_budget_raise_cap
                                    .unwrap_or(defaults::OUTPUT_BUDGET_RAISE_CAP)
                        {
                            let multiplier = self
                                .config
                                .output_budget_raise_multiplier
                                .unwrap_or(defaults::OUTPUT_BUDGET_RAISE_MULTIPLIER);
                            let cap = self
                                .config
                                .output_budget_raise_cap
                                .unwrap_or(defaults::OUTPUT_BUDGET_RAISE_CAP);
                            let raised = ((base as f64 * multiplier) as u64).min(cap as u64);
                            let mut cfg = self.config.clone();
                            cfg.max_tokens = Some(raised as u32);
                            attempt_config = Some(cfg);
                            log::warn!(
                                "OutputBudget retry: raising max_tokens {base} -> {raised} (cap {cap})"
                            );
                        } else {
                            // The raise does not apply to this retry — clear any
                            // prior raised override so a later non-OutputBudget
                            // feedback retry in the same turn runs with the base
                            // config, not the inherited raised max_tokens.
                            attempt_config = None;
                        }
                        self.append_to_memory_or_warn(
                            session_id,
                            vec![Message::user(feedback)],
                            "provider feedback retry",
                        )
                        .await;
                        continue;
                    }
                    if kind.is_retryable()
                        && attempt < self.config.max_retries.unwrap_or(defaults::MAX_RETRIES)
                        && has_partial_history
                    {
                        attempt += 1;
                        let base_delay = self
                            .config
                            .retry_base_delay_ms
                            .unwrap_or(defaults::RETRY_BASE_DELAY_MS)
                            .saturating_mul(1u64 << attempt.min(5))
                            .min(30_000);
                        // Random jitter: ±20% (0.8–1.2× multiplier) per Goose strategy
                        let jitter_factor = 0.8 + (rand::random::<f64>() * 0.4);
                        let raw_delay = if *kind == CompletionErrorKind::RateLimit {
                            extract_retry_after_ms(msg).unwrap_or(base_delay)
                        } else {
                            base_delay
                        };
                        let delay_ms = (raw_delay as f64 * jitter_factor) as u64;
                        log::warn!(
                            "Retryable error ({kind:?}), attempt {attempt}/{}. Retrying in {delay_ms}ms.",
                            self.config.max_retries.unwrap_or(defaults::MAX_RETRIES)
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                        continue;
                    }
                    // Exhausted retries — wrap the error with attempt count
                    if attempt > 0 {
                        log::warn!("Retries exhausted after {attempt} attempts: {kind:?}");
                        let retry_msg = format!("Turn failed after {attempt} retries. {msg}");
                        break Err((
                            crate::conversation::turn::TurnError::CompletionFailed {
                                msg: retry_msg,
                                kind: kind.clone(),
                            },
                            error::TurnContext {
                                last_known_history: ctx.last_known_history.clone(),
                                pre_turn_message_count: ctx.pre_turn_message_count,
                            },
                        ));
                    }
                    // Steering exhausted: the model-correctable error failed
                    // after every steering retry. The partial response text is
                    // not carried in the error path, so surface an explicit
                    // no-output statement alongside the existing guidance.
                    if is_model_correctable(kind)
                        && feedback_retries >= defaults::MAX_PROVIDER_FEEDBACK_RETRIES
                    {
                        log::warn!(
                            "Steering exhausted after {feedback_retries} feedback retries: {kind:?}"
                        );
                        let exhausted_msg = format!("{msg} No output was produced.");
                        break Err((
                            crate::conversation::turn::TurnError::CompletionFailed {
                                msg: exhausted_msg,
                                kind: kind.clone(),
                            },
                            error::TurnContext {
                                last_known_history: ctx.last_known_history.clone(),
                                pre_turn_message_count: ctx.pre_turn_message_count,
                            },
                        ));
                    }
                    break result;
                }
                Err((
                    crate::conversation::turn::TurnError::MaxTurnsExceeded {
                        msg,
                        messages,
                        max_turns,
                    },
                    ctx,
                )) => {
                    // Path A (non-cancelled): MaxTurnsError carries full chat_history.
                    // Persist only the delta (new messages from this turn) so the session
                    // remembers the failed turn without re-appending messages already in
                    // persistent storage. This runs on every MaxTurnsExceeded, before the
                    // retry decision, so completed sub-turns survive either outcome.
                    if let Some(session_id) = final_session_id {
                        // messages for MaxTurnsError is rig's full accumulated
                        // chat_history (from AgentRun::full_history()), which DOES include the
                        // current-turn user prompt and any tool call exchanges from this turn.
                        // This is unlike last_known_history (from on_completion_call's `history`
                        // parameter), which contains only prior messages. skip(pre_turn_message_count)
                        // correctly yields [user_prompt, assistant_tool_calls...] — the new messages.
                        let delta: Vec<Message> = messages
                            .iter()
                            .skip(ctx.pre_turn_message_count)
                            .cloned()
                            .collect();
                        log::debug!(
                            "Path A non-cancelled: delta_count={} pre_turn={}",
                            delta.len(),
                            ctx.pre_turn_message_count
                        );
                        if !delta.is_empty() {
                            let patched = inject_missing_tool_results(delta);
                            self.append_to_memory_or_warn(
                                session_id,
                                patched,
                                "recovered history for failed turn (path A)",
                            )
                            .await;
                        }
                    }
                    // Steering retry: exhaustion is a steering opportunity, not a
                    // hard stop. Append the model-facing steering message and
                    // re-run the turn once with a fresh tool-call budget, capped
                    // per turn by MAX_TURNS_FEEDBACK_RETRIES. Session-less turns
                    // cannot append feedback, so they fall through to the
                    // hard-error path.
                    if max_turns_feedback_retries < defaults::MAX_TURNS_FEEDBACK_RETRIES
                        && let Some(session_id) = final_session_id
                    {
                        max_turns_feedback_retries += 1;
                        let steering = build_max_turns_feedback_message(*max_turns);
                        log::warn!(
                            "Max turns exceeded, steering retry {max_turns_feedback_retries}/{}.",
                            defaults::MAX_TURNS_FEEDBACK_RETRIES
                        );
                        self.append_to_memory_or_warn(
                            session_id,
                            vec![Message::user(steering)],
                            "max-turns steering retry",
                        )
                        .await;
                        continue;
                    }
                    let msg_preview = &msg[..msg.floor_char_boundary(200)];
                    log::error!("Turn error (path A non-cancelled): {msg_preview}");
                    let user_msg = format!("Turn failed: {msg}");
                    break Err((
                        crate::conversation::turn::TurnError::MaxTurnsExceeded {
                            msg: user_msg,
                            max_turns: *max_turns,
                            messages: messages.clone(),
                        },
                        error::TurnContext {
                            last_known_history: ctx.last_known_history.clone(),
                            pre_turn_message_count: ctx.pre_turn_message_count,
                        },
                    ));
                }
                Ok(turn_result)
                    if turn_result.cancelled
                        && turn_result.cancel_reason.as_deref().is_some_and(|reason| {
                            reason.starts_with(OUTPUT_REPETITION_STOP_PREFIX)
                        }) =>
                {
                    // Path C stop-to-steering: the repetition guard stopped the
                    // stream mid-turn and the Ok-cancelled result carries the
                    // reason. Below the retry cap, publish the steering notice,
                    // deliver the repetition steering message (session append,
                    // or prompt prepend when there is no session), reset the
                    // escalation ladder (the retry starts fresh at First), and
                    // re-run the turn. A stop at the cap exhausts it and falls
                    // through to the terminal surface after the loop.
                    if rep_stop_retries < defaults::MAX_REPETITION_STOP_RETRIES {
                        rep_stop_retries += 1;
                        log::warn!(
                            "Repetition stop converted to steering retry {rep_stop_retries}/{}. ",
                            defaults::MAX_REPETITION_STOP_RETRIES
                        );
                        let _ = self
                            .tool_infra
                            .bus
                            .warning()
                            .send(WarningEvent::Message {
                                message: super::core::REPETITION_STEERING_NOTICE.to_string(),
                            })
                            .await;
                        match final_session_id {
                            Some(session_id) => {
                                self.append_to_memory_or_warn(
                                    session_id,
                                    vec![Message::user(OUTPUT_REPETITION_BACKOFF_MESSAGE)],
                                    "repetition stop steering retry",
                                )
                                .await;
                            }
                            None => {
                                // No session: carry the steering in the retry
                                // prompt (transient, nothing persisted).
                                session_less_steering =
                                    Some(OUTPUT_REPETITION_BACKOFF_MESSAGE.to_string());
                            }
                        }
                        self.tool_infra
                            .output_repetition
                            .lock()
                            .expect("output repetition mutex poisoned")
                            .reset_ladder();
                        continue;
                    }
                    break result;
                }
                Err((crate::conversation::turn::TurnError::Cancelled { msg, .. }, _))
                    if msg.starts_with(OUTPUT_REPETITION_STOP_PREFIX) =>
                {
                    // Path A stop-to-steering: the repetition guard cancelled the
                    // turn via rig's PromptCancelled. Below the retry cap,
                    // publish the steering notice, deliver the repetition
                    // steering message (session append, or prompt prepend when
                    // there is no session), reset the escalation ladder, and
                    // re-run the turn. A stop at the cap exhausts it and falls
                    // through to the terminal surface after the loop (which
                    // surfaces the stop reason).
                    if rep_stop_retries < defaults::MAX_REPETITION_STOP_RETRIES {
                        rep_stop_retries += 1;
                        log::warn!(
                            "Repetition stop converted to steering retry {rep_stop_retries}/{}. ",
                            defaults::MAX_REPETITION_STOP_RETRIES
                        );
                        let _ = self
                            .tool_infra
                            .bus
                            .warning()
                            .send(WarningEvent::Message {
                                message: super::core::REPETITION_STEERING_NOTICE.to_string(),
                            })
                            .await;
                        match final_session_id {
                            Some(session_id) => {
                                self.append_to_memory_or_warn(
                                    session_id,
                                    vec![Message::user(OUTPUT_REPETITION_BACKOFF_MESSAGE)],
                                    "repetition stop steering retry",
                                )
                                .await;
                            }
                            None => {
                                // No session: carry the steering in the retry
                                // prompt (transient, nothing persisted).
                                session_less_steering =
                                    Some(OUTPUT_REPETITION_BACKOFF_MESSAGE.to_string());
                            }
                        }
                        self.tool_infra
                            .output_repetition
                            .lock()
                            .expect("output repetition mutex poisoned")
                            .reset_ladder();
                        continue;
                    }
                    break result;
                }
                _ => break result,
            }
        }
    }
}
