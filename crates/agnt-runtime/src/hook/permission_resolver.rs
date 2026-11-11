//! Async permission resolution trait and its two implementations.
//!
//! - [`PolicyPermissionResolver`]: TTY/non-interactive mode — pure policy evaluation, returns immediately.
//! - [`InteractivePermissionResolver`]: TUI mode — publishes a `PermissionRecord::Requested` on
//!   `bus.stream()` and awaits the user's decision via a oneshot channel.

use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard, PoisonError};

use serde_json::Value as JsonValue;

use agnt_kernel::bus::{CancelEvent, OneshotTx};
use agnt_kernel::{AutoResolution, PermissionRecord, RequestId, ToolCallId as KernelCallId};

use crate::bus::{
    CancelTx, FactsTx, PermissionCard, Stream, auto_resolve_permission, request_permission,
    settle_permission,
};

use crate::protocol::event::{
    PermissionDecision as ProtocolPermissionDecision, PermissionRequestContext,
};
use crate::tools::authz::{
    AskApprovalHook, AskChoice, AskContext, PermissionsConfig, SessionGrantCache, apply_ask_choice,
    display_tool_name,
};
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::{
    AuthorizationFlowContext, AuthzOutcome, McpToolRegistry, ToolSource, ToolSourceText,
    enforce_authorization_for_tool_call, pre_authorize_fs_tool,
};
use crate::types::{CallId, ToolCall, ToolFunction, ToolName};
use agnt_kernel::BuiltinKind;

pub fn resolve_tool_source(
    name: &str,
    closures: &ClosureRegistry,
    mcp: &McpToolRegistry,
) -> ToolSource {
    if closures.get(name).is_some() {
        ToolSource::Closure
    } else if name.parse::<BuiltinKind>().is_ok() {
        ToolSource::Builtin
    } else if mcp.contains(name) {
        ToolSource::Mcp
    } else {
        ToolSource::Unknown
    }
}

/// Permission decision returned by the async resolver (and from driver to hook).
///
/// `Deny` carries the model-facing denial reason. The reason always starts
/// with `"Permission denied"` and identifies the matched rule by identity and
/// scope, so session rehydration and hook consumers can recognise a denial by
/// its prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionDecision {
    Allow,
    Deny {
        /// Model-facing denial reason; starts with `"Permission denied"`.
        reason: String,
    },
}

/// Denial reason used when a turn cancel arrives while a permission request is
/// still pending. Keeps the documented `"Permission denied"` prefix contract.
const TURN_CANCELLED_DENY_REASON: &str = "Permission denied: turn cancelled by user";

/// Denial reason used when no decision arrives within the configured decision
/// timeout. Keeps the documented `"Permission denied"` prefix contract.
const DECISION_TIMED_OUT_DENY_REASON: &str = "Permission denied: decision timed out";

// ---------------------------------------------------------------------------
// NoOp helpers for policy-only path
// ---------------------------------------------------------------------------

/// An ask hook that always denies without user interaction.
/// Used by [`PolicyPermissionResolver`] to make the "Ask" case return Deny.
struct NoOpAskHook;

impl AskApprovalHook for NoOpAskHook {
    async fn choose(
        &mut self,
        _decision: &crate::tools::authz::PermissionDecision,
        _tool_name: &str,
        _source: ToolSource,
        _args: &JsonValue,
        _ask_context: &AskContext,
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

/// Publish the policy-resolved-without-prompting record for `outcome`.
///
/// `action` is `Deny` when the gate produced a denial reason, `Allow`
/// otherwise. `reason` carries the denial reason and is `None` on allow. The
/// card content the producer computed rides the record, so the drawer joins it
/// to the call by the id it names.
fn publish_auto_resolved(
    stream: &Stream,
    facts: &FactsTx,
    outcome: &AuthzOutcome,
    card: PermissionCard,
    call_id: KernelCallId,
) {
    let action = if outcome.deny_reason.is_some() {
        AutoResolution::Deny
    } else {
        AutoResolution::Allow
    };
    auto_resolve_permission(
        stream,
        facts,
        PermissionRecord::AutoResolved {
            rule_identity: outcome.decision.matched_rule.identity.clone(),
            action,
            reason: outcome.deny_reason.clone(),
            call_id: Some(call_id),
            shows_preview: card.shows_preview,
            preview: card.content,
        },
    );
}

/// Resolve once the decision deadline passes. A `None` deadline never resolves,
/// so a disabled timeout leaves the request waiting indefinitely.
async fn wait_for_decision_timeout(deadline: Option<tokio::time::Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline).await,
        None => std::future::pending().await,
    }
}

// ---------------------------------------------------------------------------
// AsyncPermissionResolver trait
// ---------------------------------------------------------------------------

/// Resolves tool call permission decisions asynchronously.
///
/// - TUI mode: publishes the gate record on the bus and awaits the user's
///   decision via a oneshot channel.
/// - TTY mode: evaluates policy inline and returns immediately.
pub trait AsyncPermissionResolver: Clone + Send + Sync + 'static {
    fn resolve(
        &self,
        tool_name: &str,
        arguments: &str,
        tool_call_id: Option<String>,
        stream: &Stream,
        facts: &FactsTx,
        cancel: &CancelTx,
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
        stream: &Stream,
        facts: &FactsTx,
        _cancel: &CancelTx,
    ) -> impl std::future::Future<Output = PermissionDecision> + Send {
        let tool_name = tool_name.to_string();
        let arguments = arguments.to_string();
        let permissions = Arc::clone(&self.permissions);
        let session_grants = Arc::clone(&self.session_grants);
        let closure_registry = Arc::clone(&self.closure_registry);
        let mcp_registry = Arc::clone(&self.mcp_registry);
        // No card is drawn in this mode, so the policy path never reads the
        // cancel concern: only the interactive resolver awaits an answer.
        let stream = stream.clone();
        let facts = facts.clone();

        async move {
            let args_json: JsonValue = serde_json::from_str(&arguments)
                .unwrap_or(JsonValue::Object(serde_json::Map::new()));
            let call_id = tool_call_id.unwrap_or_else(|| "synthetic".to_string());
            let source = resolve_tool_source(&tool_name, &closure_registry, &mcp_registry);
            let Ok(tool_name_typed) = ToolName::new(tool_name.clone()) else {
                return PermissionDecision::Deny {
                    reason: "Permission denied: empty tool name".to_string(),
                };
            };
            let tool_call = ToolCall::new(
                CallId::from_wire(call_id),
                ToolFunction::new(tool_name_typed, args_json),
            );
            let flow_context = AuthorizationFlowContext {
                ask_context: AskContext::default(),
            };

            let outcome = enforce_authorization_for_tool_call(
                &tool_call,
                source,
                &permissions,
                session_grants,
                &flow_context,
                &mut NoOpAskHook,
            )
            .await;

            // The policy resolved without a card: no surface draws one, so the
            // record carries no preview and names the call it resolved.
            publish_auto_resolved(
                &stream,
                &facts,
                &outcome,
                PermissionCard::default(),
                KernelCallId::new(&tool_name, &arguments),
            );

            match outcome.deny_reason {
                Some(reason) => PermissionDecision::Deny { reason },
                None => PermissionDecision::Allow,
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
    /// Raw JSON arguments string of the tool call, as passed to `resolve()`.
    /// The hook only receives the parsed `JsonValue`, so the exact key the TUI
    /// uses for the pending call must be carried in from the caller.
    pub raw_arguments: String,
}

impl AskApprovalHook for AskContextCapture {
    async fn choose(
        &mut self,
        decision: &crate::tools::authz::PermissionDecision,
        tool_name: &str,
        source: ToolSource,
        args: &JsonValue,
        _ask_context: &AskContext,
    ) -> AskChoice {
        self.was_called = true;
        let call_id = KernelCallId::new(tool_name, &self.raw_arguments);
        self.captured_context = Some(PermissionRequestContext {
            tool: display_tool_name(tool_name, args),
            tool_key: call_id.as_str().to_string(),
            source: source.text().to_string(),
            mode: args
                .get("mode")
                .and_then(JsonValue::as_str)
                .map(ToString::to_string),
            matched_rule_identity: decision.matched_rule.identity.clone(),
            scope: decision.matched_rule.scope.to_string(),
            target_field: decision.matched_rule.target_field.clone(),
            pattern: decision.matched_rule.pattern.clone(),
            summary: summarize_ask_payload(args),
        });
        self.captured_auth_decision = Some(decision.clone());
        // AskContextCapture only captures context for the TUI oneshot flow.
        // The actual gate record is published on the stream in resolve().
        AskChoice::Deny
    }
}

/// Interactive permission resolver for TUI mode.
///
/// When the policy requires user confirmation ("Ask"), publishes the gate
/// record on the bus and awaits the user's decision. The orchestrator's
/// `PermissionStage` and the ACP permission bridge unblock the waiting
/// `resolve()` future by removing the request's oneshot sender from the shared
/// pending map and sending the decision on it.
///
/// **Design note (deadlock prevention):** This struct does NOT own a `Bus`. It
/// publishes on the bus handed to [`AsyncPermissionResolver::resolve`]
/// directly, so the executor's stack frame (which holds the resolver across the
/// retry loop) never keeps a sender alive that would prevent the drain loop's
/// channel from closing.
#[derive(Clone)]
pub struct InteractivePermissionResolver {
    pub pending: Arc<StdMutex<HashMap<String, OneshotTx<ProtocolPermissionDecision>>>>,
    pub permissions: Arc<PermissionsConfig>,
    pub session_grants: Arc<StdMutex<SessionGrantCache>>,
    pub closure_registry: Arc<ClosureRegistry>,
    pub mcp_registry: Arc<McpToolRegistry>,
    /// Working directory used to resolve relative paths when building the
    /// pre-authorize preview. Must match the cwd the tool handlers use —
    /// `std::env::current_dir()` is the plugin process cwd, which is frozen at
    /// plugin spawn and diverges from the Nushell session cwd after a `cd`.
    cwd: std::path::PathBuf,
}

impl InteractivePermissionResolver {
    /// Construct a new `InteractivePermissionResolver`.
    ///
    /// # Parameters
    ///
    /// - `pending`: shared map of request-id → oneshot sender, also held by the
    ///   orchestrator's `PermissionStage` and the ACP permission bridge, which
    ///   remove the sender and send the decision to unblock `resolve()`.
    /// - `permissions`: the static permission configuration.
    /// - `session_grants`: shared session grant cache for AllowAlways persistence.
    /// - `closure_registry`: registry of closure-based tools.
    /// - `mcp_registry`: registry of MCP tools.
    /// - `cwd`: the working directory used to resolve relative paths when
    ///   building the pre-authorize preview. Pass the same cwd the tool
    ///   handlers use (the runtime's engine cwd).
    pub fn new(
        pending: Arc<StdMutex<HashMap<String, OneshotTx<ProtocolPermissionDecision>>>>,
        permissions: Arc<PermissionsConfig>,
        session_grants: Arc<StdMutex<SessionGrantCache>>,
        closure_registry: Arc<ClosureRegistry>,
        mcp_registry: Arc<McpToolRegistry>,
        cwd: std::path::PathBuf,
    ) -> Self {
        Self {
            pending,
            permissions,
            session_grants,
            closure_registry,
            mcp_registry,
            cwd,
        }
    }
}

impl AsyncPermissionResolver for InteractivePermissionResolver {
    fn resolve(
        &self,
        tool_name: &str,
        arguments: &str,
        tool_call_id: Option<String>,
        stream: &Stream,
        facts: &FactsTx,
        cancel: &CancelTx,
    ) -> impl std::future::Future<Output = PermissionDecision> + Send {
        let tool_name = tool_name.to_string();
        let arguments = arguments.to_string();
        let permissions = Arc::clone(&self.permissions);
        let session_grants = Arc::clone(&self.session_grants);
        let closure_registry = Arc::clone(&self.closure_registry);
        let mcp_registry = Arc::clone(&self.mcp_registry);
        let pending = Arc::clone(&self.pending);
        let stream = stream.clone();
        let facts = facts.clone();
        let cancel = cancel.clone();
        let cwd = self.cwd.clone();

        async move {
            let args_json: JsonValue = serde_json::from_str(&arguments)
                .unwrap_or(JsonValue::Object(serde_json::Map::new()));
            let call_id = tool_call_id.unwrap_or_else(|| "synthetic".to_string());
            let source = resolve_tool_source(&tool_name, &closure_registry, &mcp_registry);
            // Build the pre-authorize preview (e.g. an edit diff) so the Ask
            // prompt below can show it before the user decides. The cwd is the
            // runtime's engine cwd — the same one the tool handlers use — so
            // relative paths resolve identically here and at execution time.
            let ask_context = tool_name
                .parse::<BuiltinKind>()
                .ok()
                .and_then(|kind| {
                    args_json
                        .as_object()
                        .and_then(|args| pre_authorize_fs_tool(Some(kind), args, &cwd))
                })
                .map(|output| output.ask_context)
                .unwrap_or_default();
            let Ok(tool_name_typed) = ToolName::new(tool_name.clone()) else {
                return PermissionDecision::Deny {
                    reason: "Permission denied: empty tool name".to_string(),
                };
            };
            let tool_call = ToolCall::new(
                CallId::from_wire(call_id),
                ToolFunction::new(tool_name_typed, args_json),
            );
            let flow_context = AuthorizationFlowContext { ask_context };

            let mut capture_hook = AskContextCapture {
                was_called: false,
                captured_context: None,
                captured_auth_decision: None,
                raw_arguments: arguments.clone(),
            };

            let outcome = {
                enforce_authorization_for_tool_call(
                    &tool_call,
                    source,
                    &permissions,
                    Arc::clone(&session_grants),
                    &flow_context,
                    &mut capture_hook,
                )
                .await
            };

            if !capture_hook.was_called {
                // Policy had an explicit Allow or Deny — no user interaction
                // needed. The record carries the card content the producer
                // computed, so the drawer joins it to the call by id.
                let card = PermissionCard {
                    shows_preview: flow_context.ask_context.shows_preview,
                    content: flow_context.ask_context.preview.clone(),
                };
                publish_auto_resolved(
                    &stream,
                    &facts,
                    &outcome,
                    card,
                    KernelCallId::new(&tool_name, &arguments),
                );
                match outcome.deny_reason {
                    Some(reason) => PermissionDecision::Deny { reason },
                    None => PermissionDecision::Allow,
                }
            } else {
                // Policy said Ask — publish the gate on the bus and await the
                // user's decision. The request's key is the call id.
                let call_id = KernelCallId::new(&tool_name, &arguments);
                let context =
                    capture_hook
                        .captured_context
                        .unwrap_or_else(|| PermissionRequestContext {
                            tool: tool_name.clone(),
                            tool_key: call_id.as_str().to_string(),
                            source: "unknown".to_string(),
                            mode: None,
                            matched_rule_identity: "unknown".to_string(),
                            scope: "unknown".to_string(),
                            target_field: None,
                            pattern: "*".to_string(),
                            summary: "→".to_string(),
                        });
                let card = PermissionCard {
                    shows_preview: flow_context.ask_context.shows_preview,
                    content: flow_context.ask_context.preview.clone(),
                };

                let (tx, mut rx) = OneshotTx::<ProtocolPermissionDecision>::channel("permission");
                // Subscribe before publishing the request: a cancel sent in
                // response to the card must be observed, and a broadcast
                // subscriber only sees events sent after it subscribes.
                let mut cancel_rx = cancel.subscribe();
                let request_id = request_permission(&stream, &facts, context, card)
                    .unwrap_or_else(|_| RequestId::mint());
                lock_pending(&pending).insert(request_id.as_str().to_string(), tx);

                // Await the TUI's decision, but stop waiting the moment the
                // turn is cancelled or the decision timeout elapses: neither a
                // confirmed cancel nor an unanswered card may block the turn.
                let timeout_deadline = permissions
                    .decision_timeout()
                    .map(|timeout| tokio::time::Instant::now() + timeout);
                let protocol_decision = loop {
                    tokio::select! {
                        decision = &mut rx => {
                            break decision.unwrap_or(ProtocolPermissionDecision::Deny);
                        }
                        event = cancel_rx.recv() => {
                            if matches!(event, Ok(CancelEvent::Requested)) {
                                lock_pending(&pending).remove(request_id.as_str());
                                let matched_rule_identity = capture_hook
                                    .captured_auth_decision
                                    .as_ref()
                                    .map(|decision| decision.matched_rule.identity.clone())
                                    .unwrap_or_default();
                                settle_permission(
                                    &stream,
                                    &facts,
                                    PermissionRecord::Cancelled {
                                        request_id: request_id.clone(),
                                        rule_identity: matched_rule_identity,
                                    },
                                );
                                return PermissionDecision::Deny {
                                    reason: TURN_CANCELLED_DENY_REASON.to_string(),
                                };
                            }
                        }
                        () = wait_for_decision_timeout(timeout_deadline) => {
                            lock_pending(&pending).remove(request_id.as_str());
                            settle_permission(
                                &stream,
                                &facts,
                                PermissionRecord::TimedOut {
                                    request_id: request_id.clone(),
                                },
                            );
                            return PermissionDecision::Deny {
                                reason: DECISION_TIMED_OUT_DENY_REASON.to_string(),
                            };
                        }
                    }
                };

                // The decision funnel: every prompted decision passes through
                // here, so one publish covers the TUI and ACP modes. The cancel
                // and timeout branches above return before this point, so they
                // publish no submitted event — no decision was submitted.
                //
                // The payload carries no origin. A TUI answer, an ACP client
                // answer, an ACP reject-always memory deny, and the ACP
                // drain-end forced deny all arrive here as a submitted
                // decision. A dropped oneshot sender breaks the loop above with
                // `Deny`, which is the accurate answer at this level.
                let matched_rule_identity = capture_hook
                    .captured_auth_decision
                    .as_ref()
                    .map(|decision| decision.matched_rule.identity.clone())
                    .unwrap_or_default();
                settle_permission(
                    &stream,
                    &facts,
                    PermissionRecord::Decided {
                        request_id: request_id.clone(),
                        decision: protocol_decision,
                        rule_identity: matched_rule_identity,
                    },
                );

                let ask_choice = match protocol_decision {
                    ProtocolPermissionDecision::AllowOnce => AskChoice::AllowOnce,
                    ProtocolPermissionDecision::AllowAlways => AskChoice::AllowAlways,
                    ProtocolPermissionDecision::Deny => AskChoice::Deny,
                };

                // Use the authz::PermissionDecision captured during the sentinel run.
                // This is the same decision the user was shown — no second evaluation needed.
                if let Some(auth_decision) = capture_hook.captured_auth_decision {
                    let mut grants = lock_session_grants(&session_grants);
                    apply_ask_choice(
                        auth_decision,
                        ask_choice,
                        &mut grants,
                        &tool_name,
                        source,
                        &tool_call.function.arguments,
                    );
                }

                match ask_choice {
                    // The sentinel run's capture hook always answers Deny, so the
                    // authz gate already built the reason from the rule that
                    // triggered the permission prompt — reuse it verbatim.
                    AskChoice::Deny => match outcome.deny_reason {
                        Some(reason) => PermissionDecision::Deny { reason },
                        // Unreachable in practice (a prompt implies a rule match);
                        // fall back to the bare denial text.
                        None => PermissionDecision::Deny {
                            reason: "Permission denied".to_string(),
                        },
                    },
                    _ => PermissionDecision::Allow,
                }
            }
        }
    }
}

// region:    --- Support

/// The pending-request map, recovered if a submitter panicked mid-insert.
fn lock_pending(
    pending: &Arc<StdMutex<HashMap<String, OneshotTx<ProtocolPermissionDecision>>>>,
) -> MutexGuard<'_, HashMap<String, OneshotTx<ProtocolPermissionDecision>>> {
    pending.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The session grant cache, recovered if a grant write panicked mid-update.
fn lock_session_grants(
    session_grants: &Arc<StdMutex<SessionGrantCache>>,
) -> MutexGuard<'_, SessionGrantCache> {
    session_grants
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

// endregion: --- Support

#[cfg(test)]
#[path = "../../test/hook/permission_resolver.rs"]
pub(crate) mod permission_resolver_test;
