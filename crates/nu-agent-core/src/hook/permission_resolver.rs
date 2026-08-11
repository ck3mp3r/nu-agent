//! Async permission resolution trait and its two implementations.
//!
//! - [`PolicyPermissionResolver`]: TTY/non-interactive mode — pure policy evaluation, returns immediately.
//! - [`InteractivePermissionResolver`]: TUI mode — sends a `PermissionRequested` event to the UI
//!   and awaits the user's decision via a oneshot channel.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use serde_json::Value as JsonValue;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

use crate::protocol::event::{
    PermissionDecision as ProtocolPermissionDecision, PermissionRequestContext, UiEvent,
};
use crate::tools::authz::{
    AskApprovalHook, AskChoice, AskContext, PermissionEventSink, PermissionsConfig,
    SessionGrantCache, apply_ask_choice, display_tool_name,
};
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::{
    AuthorizationFlowContext, McpToolRegistry, ToolSource, builtin_kinds::BuiltinKind,
    enforce_authorization_for_tool_call,
};
use crate::types::{ToolCall, ToolFunction};

fn resolve_tool_source(
    name: &str,
    closures: &ClosureRegistry,
    mcp: &McpToolRegistry,
) -> ToolSource {
    if closures.get(name).is_some() {
        ToolSource::Closure
    } else if let Ok(kind) = name.parse::<BuiltinKind>() {
        if kind.is_privileged() {
            ToolSource::BuiltinFs
        } else {
            ToolSource::Builtin
        }
    } else if mcp.contains(name) {
        ToolSource::Mcp
    } else {
        ToolSource::Unknown
    }
}

/// Permission decision returned by the async resolver (and from driver to hook).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny,
}

// ---------------------------------------------------------------------------
// Request ID generation (no uuid dep — use atomic counter)
// ---------------------------------------------------------------------------

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

fn next_request_id() -> String {
    format!(
        "perm-{:016x}",
        NEXT_REQUEST_ID.fetch_add(1, Ordering::SeqCst)
    )
}

// ---------------------------------------------------------------------------
// NoOp helpers for policy-only path
// ---------------------------------------------------------------------------

/// A no-op event sink that discards all events.
struct NoOpSink;

impl PermissionEventSink for NoOpSink {
    fn emit(&mut self, _event: UiEvent) {}
}

/// An ask hook that always denies without user interaction.
/// Used by [`PolicyPermissionResolver`] to make the "Ask" case return Deny.
struct NoOpAskHook;

#[async_trait]
impl AskApprovalHook for NoOpAskHook {
    async fn choose<S: PermissionEventSink + Send>(
        &mut self,
        _decision: &crate::tools::authz::PermissionDecision,
        _tool_name: &str,
        _source: &str,
        _args: &JsonValue,
        _ask_context: &AskContext,
        _sink: Option<&mut S>,
    ) -> AskChoice {
        AskChoice::Deny
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Summarize tool call payload for display in permission prompts.
fn summarize_ask_payload(args: &JsonValue) -> String {
    let compact = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
    let trimmed = if compact.chars().count() > 240 {
        let mut prefix = compact.chars().take(240).collect::<String>();
        prefix.push('…');
        prefix
    } else {
        compact
    };
    format!("→ {trimmed}")
}

// ---------------------------------------------------------------------------
// AsyncPermissionResolver trait
// ---------------------------------------------------------------------------

/// Resolves tool call permission decisions asynchronously.
///
/// - TUI mode: sends a `PermissionRequested` event and awaits user input.
/// - TTY mode: evaluates policy inline and returns immediately.
///
/// The `ui_tx` parameter is an optional sender for UI events. In interactive (TUI) mode,
/// the caller (AgentHook) passes its own sender so the resolver can emit
/// `PermissionRequested` events without owning a long-lived sender that would
/// prevent the drain loop's channel from closing.
pub trait AsyncPermissionResolver: Clone + Send + Sync + 'static {
    fn resolve(
        &self,
        tool_name: &str,
        arguments: &str,
        tool_call_id: Option<String>,
        ui_tx: Option<mpsc::UnboundedSender<UiEvent>>,
    ) -> impl std::future::Future<Output = PermissionDecision> + Send;
}

// ---------------------------------------------------------------------------
// PolicyPermissionResolver (TTY mode)
// ---------------------------------------------------------------------------

/// Policy-only permission resolver for non-interactive (TTY) mode.
///
/// Evaluates static permission rules and returns immediately.
/// If the policy says "Ask", the [`NoOpAskHook`] converts it to `Deny`.
#[derive(Clone)]
pub struct PolicyPermissionResolver {
    pub permissions: Arc<PermissionsConfig>,
    pub session_grants: Arc<StdMutex<SessionGrantCache>>,
    pub closure_registry: Arc<ClosureRegistry>,
    pub mcp_registry: Arc<McpToolRegistry>,
}

impl AsyncPermissionResolver for PolicyPermissionResolver {
    fn resolve(
        &self,
        tool_name: &str,
        arguments: &str,
        tool_call_id: Option<String>,
        _ui_tx: Option<mpsc::UnboundedSender<UiEvent>>,
    ) -> impl std::future::Future<Output = PermissionDecision> + Send {
        let tool_name = tool_name.to_string();
        let arguments = arguments.to_string();
        let permissions = Arc::clone(&self.permissions);
        let session_grants = Arc::clone(&self.session_grants);
        let closure_registry = Arc::clone(&self.closure_registry);
        let mcp_registry = Arc::clone(&self.mcp_registry);

        async move {
            let args_json: JsonValue = serde_json::from_str(&arguments)
                .unwrap_or(JsonValue::Object(serde_json::Map::new()));
            let call_id = tool_call_id.unwrap_or_else(|| "synthetic".to_string());
            let tool_call = ToolCall::new(call_id, ToolFunction::new(tool_name.clone(), args_json));

            let source = resolve_tool_source(&tool_name, &closure_registry, &mcp_registry);
            let flow_context = AuthorizationFlowContext {
                ask_context: AskContext::default(),
            };

            let denied = enforce_authorization_for_tool_call(
                &tool_call,
                source,
                &permissions,
                session_grants,
                &flow_context,
                &mut NoOpAskHook,
                &mut NoOpSink,
            )
            .await;

            if denied {
                PermissionDecision::Deny
            } else {
                PermissionDecision::Allow
            }
        }
    }
}

// ---------------------------------------------------------------------------
// InteractivePermissionResolver (TUI mode)
// ---------------------------------------------------------------------------

/// Capturing hook for the interactive resolver: records whether Ask was
/// triggered and saves the context needed to build `PermissionRequestContext`.
struct AskContextCapture {
    /// Whether `choose` was called (i.e. the policy said Ask).
    pub was_called: bool,
    /// The permission request context captured from the hook arguments.
    pub captured_context: Option<PermissionRequestContext>,
    /// The authz decision captured from the sentinel run (used to write the cache key).
    pub captured_auth_decision: Option<crate::tools::authz::PermissionDecision>,
}

#[async_trait]
impl AskApprovalHook for AskContextCapture {
    async fn choose<S: PermissionEventSink + Send>(
        &mut self,
        decision: &crate::tools::authz::PermissionDecision,
        tool_name: &str,
        source: &str,
        args: &JsonValue,
        ask_context: &AskContext,
        sink: Option<&mut S>,
    ) -> AskChoice {
        self.was_called = true;
        self.captured_context = Some(PermissionRequestContext {
            tool: display_tool_name(tool_name, args),
            source: source.to_string(),
            mode: args
                .get("mode")
                .and_then(JsonValue::as_str)
                .map(ToString::to_string),
            matched_rule_identity: decision.matched_rule.identity.clone(),
            scope: decision.matched_rule.scope.to_string(),
            target_field: decision.matched_rule.target_field.clone(),
            pattern: decision.matched_rule.pattern.clone(),
            summary: summarize_ask_payload(args),
            pre_authorize_display: ask_context.pre_authorize_display.clone(),
        });
        self.captured_auth_decision = Some(decision.clone());
        // Sink is ignored — AskContextCapture captures context for the TUI oneshot flow.
        // The actual UI event is emitted separately via ui_tx in resolve().
        let _ = sink;
        AskChoice::Deny
    }
}

/// Interactive permission resolver for TUI mode.
///
/// When the policy requires user confirmation ("Ask"), sends a
/// `UiEvent::PermissionRequested` and awaits the user's decision.
/// The TUI event loop must call [`InteractivePermissionResolver::submit_decision`]
/// to unblock the waiting `resolve()` future.
///
/// **Design note (deadlock prevention):** This struct intentionally does NOT own a
/// `mpsc::UnboundedSender<UiEvent>`. The sender is passed as a parameter to `resolve()`
/// by the `AgentHook` that calls it. This ensures the executor's stack frame (which holds
/// the resolver across the retry loop) never keeps a sender alive that would prevent the
/// drain loop's channel from closing — eliminating the sender-lifetime deadlock.
#[derive(Clone)]
pub struct InteractivePermissionResolver {
    pub pending: Arc<StdMutex<HashMap<String, oneshot::Sender<ProtocolPermissionDecision>>>>,
    pub permissions: Arc<PermissionsConfig>,
    pub session_grants: Arc<StdMutex<SessionGrantCache>>,
    pub closure_registry: Arc<ClosureRegistry>,
    pub mcp_registry: Arc<McpToolRegistry>,
}

impl InteractivePermissionResolver {
    /// Construct a new `InteractivePermissionResolver`.
    ///
    /// # Parameters
    ///
    /// - `pending`: shared map of request-id → oneshot sender, also held by the
    ///   orchestrator's permission poll loop so it can call `submit_decision`.
    /// - `permissions`: the static permission configuration.
    /// - `session_grants`: shared session grant cache for AllowAlways persistence.
    /// - `closure_registry`: registry of closure-based tools.
    /// - `mcp_registry`: registry of MCP tools.
    ///
    /// Note: `ui_tx` is NOT stored here. It is passed per-call via `resolve()`'s
    /// `ui_tx` parameter by the `AgentHook` that invokes the resolver.
    pub fn new(
        pending: Arc<StdMutex<HashMap<String, oneshot::Sender<ProtocolPermissionDecision>>>>,
        permissions: Arc<PermissionsConfig>,
        session_grants: Arc<StdMutex<SessionGrantCache>>,
        closure_registry: Arc<ClosureRegistry>,
        mcp_registry: Arc<McpToolRegistry>,
    ) -> Self {
        Self {
            pending,
            permissions,
            session_grants,
            closure_registry,
            mcp_registry,
        }
    }

    /// Called by the TUI event loop to resolve a pending permission request.
    pub fn submit_decision(&self, request_id: &str, decision: ProtocolPermissionDecision) {
        if let Some(tx) = self.pending.lock().unwrap().remove(request_id) {
            let _ = tx.send(decision);
        }
    }
}

impl AsyncPermissionResolver for InteractivePermissionResolver {
    fn resolve(
        &self,
        tool_name: &str,
        arguments: &str,
        tool_call_id: Option<String>,
        ui_tx: Option<mpsc::UnboundedSender<UiEvent>>,
    ) -> impl std::future::Future<Output = PermissionDecision> + Send {
        let tool_name = tool_name.to_string();
        let arguments = arguments.to_string();
        let permissions = Arc::clone(&self.permissions);
        let session_grants = Arc::clone(&self.session_grants);
        let closure_registry = Arc::clone(&self.closure_registry);
        let mcp_registry = Arc::clone(&self.mcp_registry);
        let pending = Arc::clone(&self.pending);

        async move {
            let args_json: JsonValue = serde_json::from_str(&arguments)
                .unwrap_or(JsonValue::Object(serde_json::Map::new()));
            let call_id = tool_call_id.unwrap_or_else(|| "synthetic".to_string());
            let tool_call = ToolCall::new(call_id, ToolFunction::new(tool_name.clone(), args_json));

            let source = resolve_tool_source(&tool_name, &closure_registry, &mcp_registry);
            let flow_context = AuthorizationFlowContext {
                ask_context: AskContext::default(),
            };

            let mut capture_hook = AskContextCapture {
                was_called: false,
                captured_context: None,
                captured_auth_decision: None,
            };

            let denied = {
                enforce_authorization_for_tool_call(
                    &tool_call,
                    source.clone(),
                    &permissions,
                    Arc::clone(&session_grants),
                    &flow_context,
                    &mut capture_hook,
                    &mut NoOpSink,
                )
                .await
            };

            if !capture_hook.was_called {
                // Policy had an explicit Allow or Deny — no user interaction needed.
                if denied {
                    PermissionDecision::Deny
                } else {
                    PermissionDecision::Allow
                }
            } else {
                // Policy said Ask — send event to TUI and await user decision.
                let Some(ui_tx) = ui_tx else {
                    // No UI sender available (should not happen in TUI mode).
                    // Fall back to Deny to be safe.
                    log::warn!(
                        "InteractivePermissionResolver: Ask triggered but no ui_tx provided; denying"
                    );
                    return PermissionDecision::Deny;
                };

                let request_id = next_request_id();
                let context =
                    capture_hook
                        .captured_context
                        .unwrap_or_else(|| PermissionRequestContext {
                            tool: tool_name.clone(),
                            source: "unknown".to_string(),
                            mode: None,
                            matched_rule_identity: "unknown".to_string(),
                            scope: "unknown".to_string(),
                            target_field: None,
                            pattern: "*".to_string(),
                            summary: "→".to_string(),
                            pre_authorize_display: None,
                        });

                let (tx, rx) = oneshot::channel::<ProtocolPermissionDecision>();
                pending.lock().unwrap().insert(request_id.clone(), tx);

                let _ = ui_tx.send(UiEvent::PermissionRequested {
                    request_id,
                    context,
                });

                // Await the TUI's decision. Deny if the channel is dropped.
                let protocol_decision = rx.await.unwrap_or(ProtocolPermissionDecision::Deny);

                let ask_choice = match protocol_decision {
                    ProtocolPermissionDecision::AllowOnce => AskChoice::AllowOnce,
                    ProtocolPermissionDecision::AllowAlways => AskChoice::AllowAlways,
                    ProtocolPermissionDecision::Deny => AskChoice::Deny,
                };

                // Use the authz::PermissionDecision captured during the sentinel run.
                // This is the same decision the user was shown — no second evaluation needed.
                if let Some(auth_decision) = capture_hook.captured_auth_decision {
                    let mut grants = session_grants.lock().expect("session_grants lock");
                    apply_ask_choice(
                        auth_decision,
                        ask_choice,
                        &mut grants,
                        &tool_name,
                        source.as_str(),
                        &tool_call.function.arguments,
                    );
                }

                match ask_choice {
                    AskChoice::Deny => PermissionDecision::Deny,
                    _ => PermissionDecision::Allow,
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "permission_resolver_test.rs"]
mod permission_resolver_test;
