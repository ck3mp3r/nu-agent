//! The outcome event: a settled tool result, the usage a completion reports, or
//! both, published on the record stream and the transcript.

use rig::agent::{HookContext, OutcomeAction, OutcomeEvent};
use rig::wasm_compat::WasmCompatSend;

use agnt_kernel::{
    InferenceRecord, SessionRecord, ToolCall as StreamToolCall, ToolOutcome as StreamToolOutcome,
    ToolRecord, Usage as StreamUsage,
};

use crate::hook::permission_resolver::{AsyncPermissionResolver, resolve_tool_source};
use crate::llm::{PricedOutcome, computed_cost};
use crate::protocol::events::{SystemEvent, ToolCall, ToolOutcome, Usage};
use crate::session::tool_status::{error_kind_of, status_of_result};
use crate::types::ToolName;

use super::HookChain;

impl<P: AsyncPermissionResolver> HookChain<P> {
    /// Publish the settled tool result and the completion's usage.
    pub(super) fn settle_outcome(
        &self,
        _ctx: &HookContext,
        event: OutcomeEvent<'_>,
    ) -> impl std::future::Future<Output = OutcomeAction> + WasmCompatSend {
        // `on_outcome` is unified: it fires for tool results and completions.
        // A tool result drives the transcript event, the circuit breaker, and
        // the persisted success verdict; a completion drives usage reporting.
        // Both proceed.
        let bus = self.bus.clone();

        async move {
            // Tool branch: structural success, persisted verdict, transcript
            // event, and circuit-breaker tracking.
            if let Some(result) = event.tool_result() {
                let tool_name = event.tool_name().unwrap_or_default();
                let args = event.tool_args().unwrap_or_default();
                let result_text = result.output().render();

                // Structural success: the canonical result disposition decides. A
                // non-zero `nu` exit arrives as an Error disposition (producers are
                // honest), and Refused/Skipped never count as success — no result-text
                // sniffing.
                let success = result.is_success();

                log::trace!(
                    "on_outcome: tool={tool_name} success={success} result_len={}",
                    result_text.len(),
                );
                if log::log_enabled!(log::Level::Trace) {
                    let preview = if result_text.len() > 2000 {
                        let boundary = result_text.floor_char_boundary(2000);
                        format!(
                            "{}...<truncated {} bytes>",
                            &result_text[..boundary],
                            result_text.len()
                        )
                    } else {
                        result_text.clone()
                    };
                    log::trace!("  result_body: {preview}");
                }

                // 1. The result text is what the model saw; the record carries it
                // as data. No display is derived here — the surface derives it.

                // 2. Resolve source
                let source =
                    resolve_tool_source(tool_name, &self.closure_registry, &self.mcp_registry);

                // 3. Classify error kind from raw result
                let error_kind = result.error().map(|e| e.kind().as_str().to_string());
                // The structural status, built once from the disposition and
                // carried by both the record and the transcript event.
                let status = status_of_result(result);

                // 4. Record the settled result on the call's record, so a stop
                // that drops the stream still keeps the result. The status is a
                // variant built here, from the structured disposition — never
                // from the result text.
                if let Some(call_id) = event.call_id
                    && let Ok(name) = ToolName::new(tool_name)
                {
                    self.turn_tools.record_result(
                        call_id.clone(),
                        name,
                        args,
                        crate::hook::turn_tools::SettledToolCall::new(
                            result_text.clone(),
                            success,
                            status,
                        )
                        .with_error_kind(error_kind.clone()),
                    );
                }

                // 5. Emit ToolCompleted (success was computed structurally above)
                // The pair closes on the record stream first: the disposition is
                // the enum the settle site built, and the result text is content.
                // The structured result, when the tool produced one, so a surface
                // derives the display from data instead of parsing a picture.
                let payload = serde_json::from_str::<serde_json::Value>(&result_text)
                    .ok()
                    .map(|value| value.to_string());
                let _ = bus
                    .stream()
                    .publish(SessionRecord::Tool(ToolRecord::Completed {
                        call: StreamToolCall::new(tool_name.to_string(), source, args.to_string()),
                        outcome: StreamToolOutcome {
                            status,
                            payload,
                            text: result_text.clone(),
                            message: None,
                            kind: result.error().map(|e| error_kind_of(e.kind())),
                        },
                    }));
                let call = ToolCall::new(tool_name.to_string(), source, args);
                let _ = bus
                    .ui_event()
                    .send(SystemEvent::ToolCompleted {
                        call,
                        outcome: ToolOutcome {
                            success,
                            status,
                            result: result_text,
                            error_kind,
                            message: None,
                        },
                    })
                    .await;

                // 6. Circuit breaker — track transport failures per server
                self.circuit
                    .record_result(
                        tool_name,
                        result,
                        success,
                        &self.mcp_registry,
                        bus.ui_event(),
                    )
                    .await;
            }

            // Completion branch: report usage and store the real token count for
            // the compaction threshold check.
            if let Some(response) = event.completion() {
                let usage = response.usage;
                // Any counter is enough to report the call: a wire may omit
                // `total_tokens` (ollama-cloud's OpenAI-compatible endpoint
                // does), and pricing needs only the input and output counts.
                // A usage that reports no counter at all reports nothing.
                let reports_usage = usage.input_tokens.is_some()
                    || usage.output_tokens.is_some()
                    || usage.total_tokens.is_some();
                if reports_usage {
                    let mut response_chars = 0usize;
                    let mut tool_calls = 0usize;
                    for item in response.choice.iter() {
                        match item {
                            rig::message::AssistantContent::Text(text) => {
                                response_chars += text.text.chars().count();
                            }
                            rig::message::AssistantContent::ToolCall(_) => tool_calls += 1,
                            _ => {}
                        }
                    }
                    // Store the real count: the wire's total when it reports
                    // one, else input + output — the same quantity. A wire may
                    // omit `total_tokens` (ollama-cloud's OpenAI-compatible
                    // endpoint does), and the switch gate and the threshold
                    // check both need a real count.
                    let total_tokens =
                        match (usage.total_tokens, usage.input_tokens, usage.output_tokens) {
                            (Some(total), _, _) => Some(total),
                            (None, Some(input), Some(output)) => Some(input + output),
                            _ => None,
                        };
                    if let Some(total_tokens) = total_tokens {
                        *self.lock_last_total_tokens() = Some(total_tokens);
                    }
                    let (cost, cost_complete) = match usage.cost {
                        Some(cost) => (Some(cost.total), cost.is_complete()),
                        None => match computed_cost(&self.model, &usage) {
                            PricedOutcome::Priced { total, complete } => (Some(total), complete),
                            PricedOutcome::Skipped(reason) => {
                                self.warn_unpriced_once(reason);
                                (None, false)
                            }
                        },
                    };
                    // Accumulate the turn's usage here, where the call is
                    // observed: a runner-side sum would undercount under
                    // channel lag. The session cells take the same call, so
                    // the session totals are the sum of every turn's calls.
                    self.turn_usage.add_call(
                        usage.input_tokens.unwrap_or(0),
                        usage.output_tokens.unwrap_or(0),
                        total_tokens,
                        cost,
                        cost_complete,
                    );
                    self.session_usage.add_call(
                        usage.input_tokens.unwrap_or(0),
                        usage.output_tokens.unwrap_or(0),
                        total_tokens,
                        cost,
                        cost_complete,
                    );
                    let _ = bus
                        .ui_event()
                        .send(SystemEvent::InferenceCompleted {
                            usage: Usage {
                                response_chars,
                                tool_calls,
                                input_tokens: usage.input_tokens.unwrap_or(0),
                                output_tokens: usage.output_tokens.unwrap_or(0),
                                total_tokens: usage.total_tokens,
                                cost,
                                cost_complete,
                            },
                        })
                        .await;
                    let _ = bus.stream().publish(SessionRecord::Inference(
                        InferenceRecord::Completed {
                            usage: StreamUsage {
                                input_tokens: usage.input_tokens.unwrap_or(0),
                                output_tokens: usage.output_tokens.unwrap_or(0),
                                total_tokens: usage.total_tokens,
                                cost,
                                cost_complete,
                            },
                        },
                    ));
                }
            }

            OutcomeAction::proceed()
        }
    }
}
