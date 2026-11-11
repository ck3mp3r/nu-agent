//! The session-class hook runner.
//!
//! One subscriber task per hook entry. Each task receives an event, runs the
//! hook script to completion, then receives the next — hooks run concurrently
//! with each other, but one hook never overlaps itself.

use std::path::{Path, PathBuf};

use tokio::task::JoinHandle;

use crate::bus::{LifecycleRx, LifecycleTx};
use crate::config::{HookEvent, HookEventClass, ResolvedHook};
use agnt_kernel::bus::{AgentEvent, AgentRx, AgentTx, BroadcastRx, ChannelError, LifecycleEvent};
use agnt_kernel::{
    AutoResolution, InferenceRecord, PermissionRecord, SessionRecord, ToolErrorKind, ToolRecord,
    ToolSource, ToolStatus, TurnRecord, Usage,
};

use super::context::HookAgentContext;
use super::envelope::build_envelope;
use super::process::{HookProcess, HookProcessError};

// region:    --- Runner

/// The bus channels the session-class hook runner fans out to.
///
/// Named fields, one per concern the runner's consumers read: the ordered
/// record stream and the ordered, complete lifecycle spine. The runner hands
/// each hook the concerns it declares events on, so it never needs the hub it
/// was built from.
pub struct HookSources<'a> {
    pub stream: &'a agnt_kernel::bus::Stream,
    pub lifecycle: &'a LifecycleTx,
}

/// One turn's figures, folded from that turn's ordered stream records.
///
/// The stream carries the turn's own calls and their usage; the fold is the
/// only place the `turn_completed` envelope's numbers come from, so a live
/// envelope and a replayed one agree.
#[derive(Debug, Default)]
struct TurnFold {
    tool_calls: usize,
    input_tokens: u64,
    output_tokens: u64,
    total_tokens: Option<u64>,
    cost: Option<f64>,
    cost_complete: bool,
}

impl TurnFold {
    /// Take one call's usage into the turn's sums.
    fn add_usage(&mut self, usage: &Usage) {
        self.input_tokens += usage.input_tokens;
        self.output_tokens += usage.output_tokens;
        if let Some(total) = usage.total_tokens {
            self.total_tokens = Some(self.total_tokens.unwrap_or(0) + total);
        }
        if let Some(cost) = usage.cost {
            self.cost = Some(self.cost.unwrap_or(0.0) + cost);
            self.cost_complete = usage.cost_complete;
        }
    }

    /// The `turn_completed` envelope payload.
    fn envelope(&self, duration_ms: u64) -> serde_json::Value {
        serde_json::json!({
            "tool_calls": self.tool_calls,
            "input_tokens": self.input_tokens,
            "output_tokens": self.output_tokens,
            "total_tokens": self.total_tokens,
            "cost": self.cost,
            "cost_complete": self.cost_complete,
            "duration_ms": duration_ms,
        })
    }
}

/// Spawn one observe-only subscriber task per session-class hook.
///
/// A hook that subscribes only to agent-class events is skipped: those events
/// ride a process-wide channel outside the bus and are handled by the agent
/// runner. Every task holds a clone of `context` and reads it when it builds an
/// envelope, so a persona or model switch reaches the next envelope. The
/// returned handles complete when the bus channels close.
pub fn spawn_hook_runner<P>(
    sources: HookSources<'_>,
    hooks: &[ResolvedHook],
    session_cwd: &Path,
    context: &HookAgentContext,
    process: P,
) -> Vec<JoinHandle<()>>
where
    P: HookProcess + Send + Sync + 'static + Clone,
{
    hooks
        .iter()
        .filter(|hook| {
            hook.events
                .iter()
                .any(|event| event.class() == HookEventClass::Session)
        })
        .map(|hook| {
            let hook = hook.clone();
            let cwd = session_cwd.to_path_buf();
            let process = process.clone();
            let context = context.clone();
            // Subscribe before spawning so no event published after this call
            // is missed. The lifecycle subscription is unconditional: only it
            // carries the session identity every envelope is stamped with.
            let stream_rx = needs_stream(&hook).then(|| sources.stream.subscribe());
            let lifecycle_rx = Some(sources.lifecycle.subscribe());
            tokio::spawn(async move {
                run_hook(
                    hook,
                    cwd,
                    process,
                    context,
                    HookChannels {
                        stream: stream_rx,
                        lifecycle: lifecycle_rx,
                    },
                )
                .await;
            })
        })
        .collect()
}

/// The warning text for a failed hook invocation.
pub fn failure_warning(hook: &str, event: &str, error: &HookProcessError) -> String {
    format!("hook '{hook}' failed on {event}: {error}")
}

/// Spawn one observe-only subscriber task per agent-class hook.
///
/// Spawned once per process against the standalone agent channel. The runner
/// subscribes no bus channel and tracks no session id: agent events are
/// process-scoped, so every envelope carries an empty `session_id`. A hook that
/// subscribes to no agent-class event is skipped. Every task holds a clone of
/// `context` and reads it when it builds an envelope.
pub fn spawn_agent_hook_runner<P>(
    agent_tx: &AgentTx,
    hooks: &[ResolvedHook],
    session_cwd: &Path,
    context: &HookAgentContext,
    process: P,
) -> Vec<JoinHandle<()>>
where
    P: HookProcess + Send + Sync + 'static + Clone,
{
    hooks
        .iter()
        .filter(|hook| {
            hook.events
                .iter()
                .any(|event| event.class() == HookEventClass::Agent)
        })
        .map(|hook| {
            let hook = hook.clone();
            let cwd = session_cwd.to_path_buf();
            let process = process.clone();
            let context = context.clone();
            // Subscribe before spawning so no event published after this call
            // is missed.
            let agent_rx = Some(agent_tx.subscribe());
            tokio::spawn(async move {
                run_agent_hook(hook, cwd, process, context, agent_rx).await;
            })
        })
        .collect()
}

// endregion: --- Runner

// region:    --- Support

/// Whether `hook` subscribes to the record stream.
///
/// The stream carries every tool pair and the turn's own figures; a hook that
/// declares any of them folds records.
fn needs_stream(hook: &ResolvedHook) -> bool {
    hook.events.iter().any(|event| {
        matches!(
            event,
            HookEvent::ToolStarted
                | HookEvent::ToolCompleted
                | HookEvent::TurnCompleted
                | HookEvent::PermissionRequested
                | HookEvent::PermissionDecisionTimedOut
                | HookEvent::PermissionDecisionSubmitted
                | HookEvent::PermissionDecisionIgnored
                | HookEvent::PermissionRequestCancelled
                | HookEvent::PermissionAutoResolved
        )
    })
}

/// The bus concerns one session-class hook subscribes.
///
/// Grouped so `run_hook` stays under clippy's `too_many_arguments` threshold.
/// A concern is `None` when the hook declares no event that rides it.
struct HookChannels {
    stream: Option<agnt_kernel::bus::Subscription<SessionRecord>>,
    lifecycle: Option<LifecycleRx>,
}

/// The serialized subscriber loop of one hook.
async fn run_hook<P>(
    hook: ResolvedHook,
    cwd: PathBuf,
    process: P,
    context: HookAgentContext,
    channels: HookChannels,
) where
    P: HookProcess,
{
    let HookChannels {
        stream: mut stream_rx,
        lifecycle: mut lifecycle_rx,
    } = channels;
    let mut session_id = String::new();
    let mut turn = TurnFold::default();
    loop {
        let mut invocation: Option<(&'static str, serde_json::Value)> = None;
        tokio::select! {
            biased;

            // The lifecycle arm is polled first: a session-id change must be
            // stamped before any record that rides the stream.
            event = recv_strict_optional(&mut lifecycle_rx) => match event {
                Some(LifecycleEvent::SessionStarted {
                    session_id: id,
                    hydrated,
                }) => {
                    if !id.is_empty() {
                        session_id = id.clone();
                    }
                    if hook.events.contains(&HookEvent::SessionStarted) {
                        invocation = Some((
                            "session_started",
                            serde_json::json!({"session_id": id, "hydrated": hydrated}),
                        ));
                    }
                }
                Some(LifecycleEvent::SessionEnded {
                    session_id: id,
                    input_tokens,
                    output_tokens,
                    total_tokens,
                    cost,
                    cost_complete,
                }) => {
                    if hook.events.contains(&HookEvent::SessionEnded) {
                        invocation = Some((
                            "session_ended",
                            serde_json::json!({
                                "session_id": id,
                                "input_tokens": input_tokens,
                                "output_tokens": output_tokens,
                                "total_tokens": total_tokens,
                                "cost": cost,
                                "cost_complete": cost_complete,
                            }),
                        ));
                    }
                }
                Some(LifecycleEvent::SessionSwitched { to, .. }) => {
                    session_id = to;
                }
                Some(LifecycleEvent::AgentSwitched { from, to }) => {
                    // A switch belongs to the session that made it, so it never
                    // moves the tracked id: only Started and Switched do.
                    if hook.events.contains(&HookEvent::AgentSwitched) {
                        invocation = Some((
                            "agent_switched",
                            serde_json::json!({"from": from, "to": to}),
                        ));
                    }
                }
                Some(LifecycleEvent::ModelSwitched { from, to }) => {
                    if hook.events.contains(&HookEvent::ModelSwitched) {
                        invocation = Some((
                            "model_switched",
                            serde_json::json!({"from": from, "to": to}),
                        ));
                    }
                }
                Some(LifecycleEvent::TurnStarted { prompt, task_id }) => {
                    if hook.events.contains(&HookEvent::TurnStarted) {
                        turn = TurnFold::default();
                        invocation = Some((
                            "turn_started",
                            serde_json::json!({"prompt": prompt, "task_id": task_id}),
                        ));
                    }
                }
                Some(LifecycleEvent::CompactionStarted { source }) => {
                    if hook.events.contains(&HookEvent::CompactionStarted) {
                        invocation = Some((
                            "compaction_started",
                            serde_json::json!({"source": compaction_source_text(source)}),
                        ));
                    }
                }
                Some(LifecycleEvent::CompactionCompleted { source, summary }) => {
                    if hook.events.contains(&HookEvent::CompactionCompleted) {
                        invocation = Some((
                            "compaction_completed",
                            serde_json::json!({
                                "source": compaction_source_text(source),
                                "summary_preview": summary_preview(&summary),
                            }),
                        ));
                    }
                }
                Some(LifecycleEvent::CompactionFailed { source, message }) => {
                    if hook.events.contains(&HookEvent::CompactionFailed) {
                        invocation = Some((
                            "compaction_failed",
                            serde_json::json!({
                                "source": compaction_source_text(source),
                                "message": message
                            }),
                        ));
                    }
                }
                Some(_) => {}
                None => lifecycle_rx = None,
            },
            event = recv_ring_optional(&mut stream_rx) => match event {
                Some(SessionRecord::Tool(ToolRecord::Started { call })) => {
                    turn.tool_calls += 1;
                    if hook.events.contains(&HookEvent::ToolStarted) {
                        invocation = Some((
                            "tool_started",
                            serde_json::json!({
                                "name": call.name.0,
                                "source": tool_source_text(call.source),
                                "arguments": call.arguments,
                            }),
                        ));
                    }
                }
                Some(SessionRecord::Tool(ToolRecord::Completed { call, outcome })) => {
                    if hook.events.contains(&HookEvent::ToolCompleted) {
                        invocation = Some((
                            "tool_completed",
                            serde_json::json!({
                                "name": call.name.0,
                                "source": tool_source_text(call.source),
                                "arguments": call.arguments,
                                "success": outcome.status == ToolStatus::Success,
                                "error_kind": outcome.kind.map(error_kind_text),
                                "message": outcome.message,
                            }),
                        ));
                    }
                }
                Some(SessionRecord::Permission(record)) => {
                    invocation = permission_invocation(&hook, &record);
                }
                Some(SessionRecord::Inference(InferenceRecord::Completed { usage })) => {
                    turn.add_usage(&usage);
                }
                Some(SessionRecord::Turn(TurnRecord::Completed { duration_ms })) => {
                    if hook.events.contains(&HookEvent::TurnCompleted) {
                        invocation = Some(("turn_completed", turn.envelope(duration_ms)));
                    }
                    turn = TurnFold::default();
                }
                Some(_) => {}
                None => stream_rx = None,
            },
        }

        if let Some((event, payload)) = invocation {
            let envelope = build_envelope(event, &session_id, &context, payload);
            if let Err(error) = process
                .run(&hook.script, event, &envelope, &cwd, hook.timeout)
                .await
            {
                log::warn!("{}", failure_warning(&hook.name, event, &error));
            }
        }

        if stream_rx.is_none() && lifecycle_rx.is_none() {
            break;
        }
    }
}

/// The serialized subscriber loop of one agent-class hook.
///
/// No session-id tracking: agent events are process-scoped, so every envelope
/// carries the constant empty string.
async fn run_agent_hook<P>(
    hook: ResolvedHook,
    cwd: PathBuf,
    process: P,
    context: HookAgentContext,
    mut agent_rx: Option<AgentRx>,
) where
    P: HookProcess,
{
    loop {
        let mut invocation: Option<(&'static str, serde_json::Value)> = None;
        tokio::select! {
            event = recv_optional(&mut agent_rx) => match event {
                Some(AgentEvent::Started { mode }) => {
                    if hook.events.contains(&HookEvent::AgentStarted) {
                        invocation = Some(("agent_started", serde_json::json!({"mode": mode})));
                    }
                }
                Some(AgentEvent::Shutdown { reason }) => {
                    if hook.events.contains(&HookEvent::AgentShutdown) {
                        invocation =
                            Some(("agent_shutdown", serde_json::json!({"reason": reason})));
                    }
                }
                None => agent_rx = None,
            },
        }

        if let Some((event, payload)) = invocation {
            let envelope = build_envelope(event, "", &context, payload);
            if let Err(error) = process
                .run(&hook.script, event, &envelope, &cwd, hook.timeout)
                .await
            {
                log::warn!("{}", failure_warning(&hook.name, event, &error));
            }
        }

        if agent_rx.is_none() {
            break;
        }
    }
}

/// Receive the next event, or pend forever when the channel is not subscribed.
///
/// `None` means the channel closed; the caller drops that arm from the loop.
async fn recv_optional<T: Clone + Send + 'static>(rx: &mut Option<BroadcastRx<T>>) -> Option<T> {
    let Some(rx) = rx else {
        return std::future::pending().await;
    };
    loop {
        match rx.recv().await {
            Ok(event) => return Some(event),
            Err(ChannelError::Lagged { .. }) => continue,
            Err(_) => return None,
        }
    }
}

/// The same await for a ring subscription: a lagged reader re-syncs at the tail
/// and resumes in receipt order.
async fn recv_ring_optional<T: Clone>(
    rx: &mut Option<agnt_kernel::bus::Subscription<T>>,
) -> Option<T> {
    let Some(rx) = rx else {
        return std::future::pending().await;
    };
    loop {
        match rx.recv().await {
            Ok(event) => return Some(event),
            Err(agnt_kernel::bus::ReceiveError::Lagged(_)) => continue,
            Err(_) => return None,
        }
    }
}

/// The same await for a strict subscription: nothing is ever skipped, so a
/// closed fan-out is the only terminal read.
async fn recv_strict_optional<T: Clone>(
    rx: &mut Option<agnt_kernel::bus::StrictSubscription<T>>,
) -> Option<T> {
    let Some(rx) = rx else {
        return std::future::pending().await;
    };
    rx.recv().await.ok()
}

/// The envelope of the permission record, when the hook declares one.
///
/// Every field comes from the record the gate published, field for field; the
/// labels the envelopes carry are derived here, at the drawer.
fn permission_invocation(
    hook: &ResolvedHook,
    record: &PermissionRecord,
) -> Option<(&'static str, serde_json::Value)> {
    match record {
        PermissionRecord::Requested {
            request_id,
            call_id,
            tool,
            source,
            rule_identity,
            scope,
            pattern,
            target_field,
            mode,
            summary,
            ..
        } => hook
            .events
            .contains(&HookEvent::PermissionRequested)
            .then(|| {
                (
                    "permission_requested",
                    serde_json::json!({
                        "request_id": request_id.as_str(),
                        "tool": tool,
                        "tool_key": call_id.as_str(),
                        "source": source,
                        "mode": mode,
                        "matched_rule_identity": rule_identity,
                        "scope": scope,
                        "target_field": target_field,
                        "pattern": pattern,
                        "summary": summary,
                    }),
                )
            }),
        PermissionRecord::Decided {
            request_id,
            decision,
            rule_identity,
        } => hook
            .events
            .contains(&HookEvent::PermissionDecisionSubmitted)
            .then(|| {
                (
                    "permission_decision_submitted",
                    serde_json::json!({
                        "request_id": request_id.as_str(),
                        "decision": decision.as_str(),
                        "matched_rule_identity": rule_identity,
                    }),
                )
            }),
        PermissionRecord::TimedOut { request_id } => hook
            .events
            .contains(&HookEvent::PermissionDecisionTimedOut)
            .then(|| {
                (
                    "permission_decision_timed_out",
                    serde_json::json!({"request_id": request_id.as_str()}),
                )
            }),
        PermissionRecord::Ignored { request_id, reason } => hook
            .events
            .contains(&HookEvent::PermissionDecisionIgnored)
            .then(|| {
                (
                    "permission_decision_ignored",
                    serde_json::json!({
                        "request_id": request_id.as_str(),
                        "reason": reason,
                    }),
                )
            }),
        PermissionRecord::Cancelled {
            request_id,
            rule_identity,
        } => hook
            .events
            .contains(&HookEvent::PermissionRequestCancelled)
            .then(|| {
                (
                    "permission_request_cancelled",
                    serde_json::json!({
                        "request_id": request_id.as_str(),
                        "matched_rule_identity": rule_identity,
                    }),
                )
            }),
        PermissionRecord::AutoResolved {
            rule_identity,
            action,
            reason,
            ..
        } => hook
            .events
            .contains(&HookEvent::PermissionAutoResolved)
            .then(|| {
                (
                    "permission_auto_resolved",
                    serde_json::json!({
                        "matched_rule_identity": rule_identity,
                        "action": auto_resolution_text(*action),
                        "reason": reason,
                    }),
                )
            }),
    }
}

/// The text a tool source prints in an envelope.
fn tool_source_text(source: ToolSource) -> &'static str {
    match source {
        ToolSource::Closure => "closure",
        ToolSource::Mcp => "mcp",
        ToolSource::Builtin => "builtin",
        ToolSource::Unknown => "unknown",
    }
}

/// The text a tool-error kind prints in an envelope.
fn error_kind_text(kind: ToolErrorKind) -> &'static str {
    match kind {
        ToolErrorKind::InvalidArgs => "invalid_args",
        ToolErrorKind::Timeout => "timeout",
        ToolErrorKind::Cancelled => "cancelled",
        ToolErrorKind::NotFound => "not_found",
        ToolErrorKind::PermissionDenied => "permission_denied",
        ToolErrorKind::RateLimited => "rate_limited",
        ToolErrorKind::Provider => "provider",
        ToolErrorKind::Network => "network",
        ToolErrorKind::Other => "other",
        ToolErrorKind::Denied => "denied",
    }
}

/// The text a policy resolution prints in an envelope.
fn auto_resolution_text(action: AutoResolution) -> &'static str {
    match action {
        AutoResolution::Allow => "allow",
        AutoResolution::Deny => "deny",
    }
}

/// The text a compaction source prints in an envelope.
fn compaction_source_text(source: agnt_kernel::CompactionSource) -> &'static str {
    match source {
        agnt_kernel::CompactionSource::Auto => "auto",
        agnt_kernel::CompactionSource::Manual => "slash",
    }
}

/// The first 200 characters of a compaction summary.
fn summary_preview(summary: &str) -> String {
    summary.chars().take(200).collect()
}

// endregion: --- Support
