//! HookDriver — bridges async hook events to sync ProgressUi

use serde_json::Value as JsonValue;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::permission_bridge::resolve_tool_source;
use super::prompt_hook::CopilotPromptHook;
use super::types::{HookEvent, PermissionDecision};
use crate::agent::protocol::contracts::ProgressUi;
use crate::agent::protocol::event::UiEvent;
use crate::agent::tools::authz::PermissionEventSink;
use crate::agent::tools::handler::McpToolRegistry;
use crate::tools::closure::ClosureRegistry;

/// Resolves tool permissions. Provided by the caller.
pub trait PermissionResolver {
    /// Decide whether a tool call should be allowed.
    /// Called synchronously on the driver thread.
    fn resolve<S: PermissionEventSink>(
        &mut self,
        tool_name: &str,
        arguments: &str,
        tool_call_id: Option<String>,
        sink: &mut S,
    ) -> PermissionDecision;
}

/// Wrapper that implements PermissionEventSink for the driver's UI.
/// This ensures permission events reach the UI immediately.
struct DriverPermissionSink<'a, U: ProgressUi> {
    ui: &'a mut U,
}

impl<U: ProgressUi> PermissionEventSink for DriverPermissionSink<'_, U> {
    fn emit(&mut self, event: UiEvent) {
        self.ui.emit(&event);
    }
}

/// Extract display data from tool result JSON.
/// Returns None if result is not valid JSON or doesn't contain displayable data.
fn extract_display_from_result(
    tool_name: &str,
    result: &str,
) -> Option<crate::agent::protocol::event::ToolDisplay> {
    // Try to parse result as JSON
    let json: JsonValue = serde_json::from_str(result).ok()?;

    // Use the same logic as v1 path
    crate::agent::tools::handler::build_direct_tool_display(tool_name, &json)
}

/// Bridges async PromptHook events to sync ProgressUi.
pub struct HookDriver {
    event_rx: mpsc::UnboundedReceiver<HookEvent>,
    tool_call_count: usize,
}

impl HookDriver {
    /// Create a matched (hook, driver) pair.
    pub fn new(cancel_token: CancellationToken) -> (CopilotPromptHook, Self) {
        let (tx, rx) = mpsc::unbounded_channel();
        let hook = CopilotPromptHook::new(tx, cancel_token);
        let driver = Self {
            event_rx: rx,
            tool_call_count: 0,
        };
        (hook, driver)
    }

    /// Get the total number of tool calls executed during this turn.
    /// This count is incremented each time a ToolEnd event is processed.
    pub fn tool_call_count(&self) -> usize {
        self.tool_call_count
    }

    /// Run the driver loop, blocking until the hook's channel closes
    /// (i.e., agent.prompt() completes or the hook is dropped).
    ///
    /// Maps HookEvents to UiEvents and resolves permissions.
    pub(crate) fn run_until_complete<U, P>(
        &mut self,
        ui: &mut U,
        permissions: &mut P,
        closure_registry: &ClosureRegistry,
        mcp_registry: &McpToolRegistry,
        cancel_token: &CancellationToken,
    ) where
        U: ProgressUi,
        P: PermissionResolver,
    {
        loop {
            // Check if UI requested cancellation
            if ui.take_cancel_requested() {
                // Cancel the token so the hook sees it
                cancel_token.cancel();
            }

            match self.event_rx.try_recv() {
                Ok(event) => {
                    self.handle_event(event, ui, permissions, closure_registry, mcp_registry)
                }
                Err(mpsc::error::TryRecvError::Empty) => {
                    // No events yet — brief sleep to avoid busy-spin
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    // Hook dropped — agent loop finished
                    break;
                }
            }
        }
        ui.flush();
    }

    fn handle_event<U, P>(
        &mut self,
        event: HookEvent,
        ui: &mut U,
        permissions: &mut P,
        closure_registry: &ClosureRegistry,
        mcp_registry: &McpToolRegistry,
    ) where
        U: ProgressUi,
        P: PermissionResolver,
    {
        match event {
            HookEvent::LlmStart => {
                ui.emit(&UiEvent::LlmStart);
            }
            HookEvent::LlmEnd {
                response_chars,
                tool_calls,
                input_tokens,
                output_tokens,
                total_tokens,
            } => {
                ui.emit(&UiEvent::LlmEnd {
                    response_chars,
                    tool_calls,
                    input_tokens,
                    output_tokens,
                    total_tokens,
                });
            }
            HookEvent::ToolStart { name, arguments } => {
                let resolved_source = resolve_tool_source(&name, closure_registry, mcp_registry)
                    .as_str()
                    .to_string();
                ui.emit(&UiEvent::ToolStart {
                    name,
                    source: resolved_source,
                    arguments,
                });
            }
            HookEvent::ToolEnd {
                name,
                arguments,
                success,
                result,
                error_kind,
                message,
            } => {
                // Increment tool call counter
                self.tool_call_count += 1;

                let resolved_source = resolve_tool_source(&name, closure_registry, mcp_registry)
                    .as_str()
                    .to_string();

                // Extract display data from result if present
                let display = extract_display_from_result(&name, &result);

                ui.emit(&UiEvent::ToolEnd {
                    name,
                    source: resolved_source,
                    arguments,
                    success,
                    result,
                    display,
                    error_kind,
                    message,
                });
            }
            HookEvent::TextDelta {
                delta,
                aggregated: _,
            } => {
                ui.emit(&UiEvent::AssistantMessage { text: delta });
            }
            HookEvent::AskPermission {
                tool_name,
                arguments,
                tool_call_id,
                responder,
            } => {
                let mut sink = DriverPermissionSink { ui };
                let decision = permissions.resolve(&tool_name, &arguments, tool_call_id, &mut sink);
                let _ = responder.send(decision);
            }
            HookEvent::DoomLoopDetected { tool_name, count } => {
                ui.emit(&UiEvent::Warning {
                    message: format!(
                        "Doom loop detected: '{}' called {} times with identical arguments. Breaking tool loop.",
                        tool_name, count
                    ),
                });
            }
        }
    }
}

#[cfg(test)]
#[path = "driver_test.rs"]
mod driver_test;
