//! CopilotPromptHook implementation — bridges async agent loop with sync UI/permission system

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use rig::agent::{HookAction, PromptHook, ToolCallHookAction};
use rig::completion::GetTokenUsage;
use rig::completion::request::CompletionModel;
use rig::message::Message;

use super::types::{HookEvent, PermissionDecision};

const DOOM_LOOP_THRESHOLD: usize = 5;

/// Cancellation reason string used when user cancels operations.
/// This const ensures consistency across all cancellation points.
pub const CANCELLATION_REASON: &str = "Cancelled by user";

/// Tracks recent tool call signatures for doom loop detection
#[derive(Debug, Clone, Default)]
struct DoomLoopState {
    recent_signatures: Vec<(String, String)>, // (tool_name, arguments)
}

impl DoomLoopState {
    /// Returns Some(tool_name) if doom loop detected
    fn check_and_record(&mut self, name: &str, args: &str) -> Option<String> {
        self.recent_signatures
            .push((name.to_string(), args.to_string()));

        if self.recent_signatures.len() < DOOM_LOOP_THRESHOLD {
            return None;
        }

        let last_n = &self.recent_signatures[self.recent_signatures.len() - DOOM_LOOP_THRESHOLD..];
        let first = &last_n[0];
        if last_n.iter().all(|sig| sig == first) {
            Some(name.to_string())
        } else {
            None
        }
    }
}

/// Async PromptHook that communicates with the sync UI via channels.
#[derive(Clone)]
pub struct CopilotPromptHook {
    event_tx: mpsc::UnboundedSender<HookEvent>,
    doom_state: Arc<Mutex<DoomLoopState>>,
    cancel_token: CancellationToken,
    last_total_tokens: Arc<AtomicU64>,
}

impl CopilotPromptHook {
    pub fn new(
        event_tx: mpsc::UnboundedSender<HookEvent>,
        cancel_token: CancellationToken,
        last_total_tokens: Arc<AtomicU64>,
    ) -> Self {
        Self {
            event_tx,
            doom_state: Arc::new(Mutex::new(DoomLoopState::default())),
            cancel_token,
            last_total_tokens,
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancel_token.is_cancelled()
    }

    fn send_event(&self, event: HookEvent) {
        // Ignore send errors — means driver dropped (agent loop ending)
        let _ = self.event_tx.send(event);
    }
}

impl<M> PromptHook<M> for CopilotPromptHook
where
    M: CompletionModel,
{
    async fn on_completion_call(&self, _prompt: &Message, _history: &[Message]) -> HookAction {
        if self.is_cancelled() {
            return HookAction::Terminate {
                reason: CANCELLATION_REASON.to_string(),
            };
        }
        self.send_event(HookEvent::LlmStart);
        HookAction::Continue
    }

    async fn on_tool_call(
        &self,
        tool_name: &str,
        tool_call_id: Option<String>,
        _internal_call_id: &str,
        args: &str,
    ) -> ToolCallHookAction {
        // 1. Check cancellation
        if self.is_cancelled() {
            return ToolCallHookAction::Terminate {
                reason: CANCELLATION_REASON.to_string(),
            };
        }

        // 2. Check doom loop
        {
            let mut state = self.doom_state.lock().unwrap();
            if let Some(tool) = state.check_and_record(tool_name, args) {
                self.send_event(HookEvent::DoomLoopDetected {
                    tool_name: tool.clone(),
                    count: DOOM_LOOP_THRESHOLD,
                });
                return ToolCallHookAction::Terminate {
                    reason: format!(
                        "Doom loop detected: '{}' called {} times with identical arguments",
                        tool, DOOM_LOOP_THRESHOLD
                    ),
                };
            }
        }

        // 3. Announce tool call (visible in transcript immediately)
        self.send_event(HookEvent::ToolStart {
            name: tool_name.to_string(),
            arguments: args.to_string(),
        });

        // 4. Ask permission via channel
        let (tx, rx) = oneshot::channel();
        self.send_event(HookEvent::AskPermission {
            tool_name: tool_name.to_string(),
            arguments: args.to_string(),
            tool_call_id,
            responder: tx,
        });

        // Block until driver responds
        match rx.await {
            Ok(PermissionDecision::Allow) => ToolCallHookAction::Continue,
            Ok(PermissionDecision::Deny) => {
                self.send_event(HookEvent::ToolEnd {
                    name: tool_name.to_string(),
                    arguments: args.to_string(),
                    success: false,
                    result: String::new(),
                    error_kind: None,
                    message: Some("Permission denied".to_string()),
                });
                ToolCallHookAction::Skip {
                    reason: "Permission denied".to_string(),
                }
            }
            Err(_) => {
                // Channel closed — driver dropped, terminate
                ToolCallHookAction::Terminate {
                    reason: "Hook driver disconnected".to_string(),
                }
            }
        }
    }

    async fn on_tool_result(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        _internal_call_id: &str,
        args: &str,
        result: &str,
    ) -> HookAction {
        self.send_event(HookEvent::ToolEnd {
            name: tool_name.to_string(),
            arguments: args.to_string(),
            success: true, // rig only calls on_tool_result for successful results
            result: result.to_string(),
            error_kind: None,
            message: None,
        });
        HookAction::Continue
    }

    async fn on_text_delta(&self, delta: &str, aggregated: &str) -> HookAction {
        if self.is_cancelled() {
            return HookAction::Terminate {
                reason: CANCELLATION_REASON.to_string(),
            };
        }
        log::trace!(
            "hook: TextDelta delta_len={} aggregated_len={}",
            delta.len(),
            aggregated.len()
        );
        self.send_event(HookEvent::TextDelta {
            delta: delta.to_string(),
            aggregated: aggregated.to_string(),
        });
        HookAction::Continue
    }

    async fn on_stream_completion_response_finish(
        &self,
        _prompt: &Message,
        response: &M::StreamingResponse,
    ) -> HookAction {
        if let Some(usage) = response.token_usage() {
            self.last_total_tokens
                .store(usage.total_tokens, Ordering::Relaxed);
            self.send_event(HookEvent::LlmEnd {
                response_chars: 0,
                tool_calls: 0,
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
            });
        }
        HookAction::Continue
    }

    // Use default for on_tool_call_delta
}

#[cfg(test)]
#[path = "prompt_hook_test.rs"]
mod prompt_hook_test;
