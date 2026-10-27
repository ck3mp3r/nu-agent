//! The session-class hook runner.
//!
//! One subscriber task per hook entry. Each task receives an event, runs the
//! hook script to completion, then receives the next — hooks run concurrently
//! with each other, but one hook never overlaps itself.

use std::path::{Path, PathBuf};

use tokio::task::JoinHandle;

use crate::bus::{
    AgentEvent, AgentRx, AgentTx, BroadcastRx, Bus, ChannelError, CompactionEvent, CompactionRx,
    SessionEvent, SessionRx, TurnEvent, TurnRx, UiEventRx,
};
use crate::config::{HookEvent, HookEventClass, ResolvedHook};
use crate::protocol::event::UiEvent;

use super::context::HookAgentContext;
use super::envelope::build_envelope;
use super::process::{HookProcess, HookProcessError};

// region:    --- Runner

/// Spawn one observe-only subscriber task per session-class hook.
///
/// A hook that subscribes only to agent-class events is skipped: those events
/// ride a process-wide channel outside the bus and are handled by the agent
/// runner. Every task holds a clone of `context` and reads it when it builds an
/// envelope, so a persona or model switch reaches the next envelope. The
/// returned handles complete when the bus channels close.
pub fn spawn_hook_runner<P>(
    bus: &Bus,
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
            // is missed.
            let turn_rx = needs_turn(&hook).then(|| bus.turn().subscribe());
            let ui_rx = needs_ui(&hook).then(|| bus.ui_event().subscribe());
            let compaction_rx = needs_compaction(&hook).then(|| bus.compaction().subscribe());
            let session_rx = Some(bus.session().subscribe());
            tokio::spawn(async move {
                run_hook(
                    hook,
                    cwd,
                    process,
                    context,
                    HookChannels {
                        turn: turn_rx,
                        ui: ui_rx,
                        compaction: compaction_rx,
                        session: session_rx,
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

/// Whether `hook` subscribes to a turn event.
fn needs_turn(hook: &ResolvedHook) -> bool {
    hook.events
        .iter()
        .any(|event| matches!(event, HookEvent::TurnStarted | HookEvent::TurnCompleted))
}

/// Whether `hook` subscribes to a ui event.
fn needs_ui(hook: &ResolvedHook) -> bool {
    hook.events.iter().any(|event| {
        matches!(
            event,
            HookEvent::ToolStarted
                | HookEvent::ToolCompleted
                | HookEvent::PermissionRequested
                | HookEvent::PermissionDecisionTimedOut
                | HookEvent::PermissionDecisionSubmitted
                | HookEvent::PermissionDecisionIgnored
                | HookEvent::PermissionRequestCancelled
                | HookEvent::PermissionAutoResolved
        )
    })
}

/// Whether `hook` subscribes to a compaction event.
///
/// Compaction events ride their own bus channel: no production code publishes
/// them as `UiEvent`s, so a compaction hook that subscribed `ui_event` would
/// never fire.
fn needs_compaction(hook: &ResolvedHook) -> bool {
    hook.events.iter().any(|event| {
        matches!(
            event,
            HookEvent::CompactionStarted
                | HookEvent::CompactionCompleted
                | HookEvent::CompactionFailed
        )
    })
}

/// The bus channels one session-class hook subscribes.
///
/// Grouped so `run_hook` stays under clippy's `too_many_arguments` threshold.
/// A channel is `None` when the hook declares no event that rides it.
struct HookChannels {
    turn: Option<TurnRx>,
    ui: Option<UiEventRx>,
    compaction: Option<CompactionRx>,
    session: Option<SessionRx>,
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
        turn: mut turn_rx,
        ui: mut ui_rx,
        compaction: mut compaction_rx,
        session: mut session_rx,
    } = channels;
    let mut session_id = String::new();
    loop {
        let mut invocation: Option<(&'static str, serde_json::Value)> = None;
        tokio::select! {
            biased;

            // The session arm is polled first: a session-id change must be
            // stamped before any event that rides another channel.
            event = recv_optional(&mut session_rx) => match event {
                Some(SessionEvent::Started {
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
                Some(SessionEvent::Ended {
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
                Some(SessionEvent::Switched { to_session_id, .. }) => {
                    session_id = to_session_id;
                }
                Some(SessionEvent::AgentSwitched { from, to }) => {
                    // A switch belongs to the session that made it, so it never
                    // moves the tracked id: only Started and Switched do.
                    if hook.events.contains(&HookEvent::AgentSwitched) {
                        invocation = Some((
                            "agent_switched",
                            serde_json::json!({"from": from, "to": to}),
                        ));
                    }
                }
                Some(SessionEvent::ModelSwitched { from, to }) => {
                    if hook.events.contains(&HookEvent::ModelSwitched) {
                        invocation = Some((
                            "model_switched",
                            serde_json::json!({"from": from, "to": to}),
                        ));
                    }
                }
                None => session_rx = None,
            },
            event = recv_optional(&mut turn_rx) => match event {
                Some(TurnEvent::Started { prompt, task_id }) => {
                    if hook.events.contains(&HookEvent::TurnStarted) {
                        invocation = Some((
                            "turn_started",
                            serde_json::json!({"prompt": prompt, "task_id": task_id}),
                        ));
                    }
                }
                Some(TurnEvent::Completed {
                    tool_calls,
                    input_tokens,
                    output_tokens,
                    total_tokens,
                    cost,
                    cost_complete,
                    duration_ms,
                }) => {
                    if hook.events.contains(&HookEvent::TurnCompleted) {
                        invocation = Some((
                            "turn_completed",
                            serde_json::json!({
                                "tool_calls": tool_calls,
                                "input_tokens": input_tokens,
                                "output_tokens": output_tokens,
                                "total_tokens": total_tokens,
                                "cost": cost,
                                "cost_complete": cost_complete,
                                "duration_ms": duration_ms,
                            }),
                        ));
                    }
                }
                Some(_) => {}
                None => turn_rx = None,
            },
            event = recv_optional(&mut ui_rx) => match event {
                Some(UiEvent::ToolStarted {
                    name,
                    source,
                    arguments,
                    ..
                }) => {
                    if hook.events.contains(&HookEvent::ToolStarted) {
                        invocation = Some((
                            "tool_started",
                            serde_json::json!({
                                "name": name,
                                "source": source,
                                "arguments": arguments,
                            }),
                        ));
                    }
                }
                Some(UiEvent::ToolCompleted {
                    name,
                    source,
                    arguments,
                    success,
                    error_kind,
                    message,
                    ..
                }) => {
                    if hook.events.contains(&HookEvent::ToolCompleted) {
                        invocation = Some((
                            "tool_completed",
                            serde_json::json!({
                                "name": name,
                                "source": source,
                                "arguments": arguments,
                                "success": success,
                                "error_kind": error_kind,
                                "message": message,
                            }),
                        ));
                    }
                }
                Some(UiEvent::PermissionRequested {
                    request_id,
                    context,
                }) => {
                    if hook.events.contains(&HookEvent::PermissionRequested) {
                        invocation = Some((
                            "permission_requested",
                            serde_json::json!({
                                "request_id": request_id,
                                "tool": context.tool,
                                "tool_key": context.tool_key,
                                "source": context.source,
                                "mode": context.mode,
                                "matched_rule_identity": context.matched_rule_identity,
                                "scope": context.scope,
                                "target_field": context.target_field,
                                "pattern": context.pattern,
                                "summary": context.summary,
                            }),
                        ));
                    }
                }
                Some(UiEvent::PermissionDecisionTimedOut { request_id }) => {
                    if hook.events.contains(&HookEvent::PermissionDecisionTimedOut) {
                        invocation = Some((
                            "permission_decision_timed_out",
                            serde_json::json!({"request_id": request_id}),
                        ));
                    }
                }
                Some(UiEvent::PermissionDecisionSubmitted {
                    request_id,
                    decision,
                    matched_rule_identity,
                }) => {
                    if hook.events.contains(&HookEvent::PermissionDecisionSubmitted) {
                        invocation = Some((
                            "permission_decision_submitted",
                            serde_json::json!({
                                "request_id": request_id,
                                "decision": decision.as_str(),
                                "matched_rule_identity": matched_rule_identity,
                            }),
                        ));
                    }
                }
                Some(UiEvent::PermissionDecisionIgnored { request_id, reason }) => {
                    if hook.events.contains(&HookEvent::PermissionDecisionIgnored) {
                        invocation = Some((
                            "permission_decision_ignored",
                            serde_json::json!({
                                "request_id": request_id,
                                "reason": reason,
                            }),
                        ));
                    }
                }
                Some(UiEvent::PermissionRequestCancelled {
                    request_id,
                    matched_rule_identity,
                }) => {
                    if hook.events.contains(&HookEvent::PermissionRequestCancelled) {
                        invocation = Some((
                            "permission_request_cancelled",
                            serde_json::json!({
                                "request_id": request_id,
                                "matched_rule_identity": matched_rule_identity,
                            }),
                        ));
                    }
                }
                Some(UiEvent::PermissionAutoResolved {
                    matched_rule_identity,
                    action,
                    reason,
                }) => {
                    if hook.events.contains(&HookEvent::PermissionAutoResolved) {
                        invocation = Some((
                            "permission_auto_resolved",
                            serde_json::json!({
                                "matched_rule_identity": matched_rule_identity,
                                "action": action,
                                "reason": reason,
                            }),
                        ));
                    }
                }
                Some(_) => {}
                None => ui_rx = None,
            },
            event = recv_optional(&mut compaction_rx) => match event {
                Some(CompactionEvent::Started { source }) => {
                    if hook.events.contains(&HookEvent::CompactionStarted) {
                        invocation = Some((
                            "compaction_started",
                            serde_json::json!({"source": source}),
                        ));
                    }
                }
                Some(CompactionEvent::Completed {
                    source,
                    summary_preview,
                    ..
                }) => {
                    if hook.events.contains(&HookEvent::CompactionCompleted) {
                        invocation = Some((
                            "compaction_completed",
                            serde_json::json!({
                                "source": source,
                                "summary_preview": summary_preview,
                            }),
                        ));
                    }
                }
                Some(CompactionEvent::Failed { source, message }) => {
                    if hook.events.contains(&HookEvent::CompactionFailed) {
                        invocation = Some((
                            "compaction_failed",
                            serde_json::json!({"source": source, "message": message}),
                        ));
                    }
                }
                Some(_) => {}
                None => compaction_rx = None,
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

        if turn_rx.is_none() && ui_rx.is_none() && compaction_rx.is_none() && session_rx.is_none() {
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

// endregion: --- Support
