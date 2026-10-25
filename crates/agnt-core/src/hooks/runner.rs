//! The session-class hook runner.
//!
//! One subscriber task per hook entry. Each task receives an event, runs the
//! hook script to completion, then receives the next — hooks run concurrently
//! with each other, but one hook never overlaps itself.

use std::path::{Path, PathBuf};

use tokio::task::JoinHandle;

use crate::bus::{
    AgentEvent, AgentRx, AgentTx, BroadcastRx, Bus, ChannelError, SessionEvent, SessionRx,
    TurnEvent, TurnRx, UiEventRx,
};
use crate::config::{HookEvent, HookEventClass, ResolvedHook};
use crate::protocol::event::UiEvent;

use super::envelope::build_envelope;
use super::process::{HookProcess, HookProcessError};

// region:    --- Runner

/// Spawn one observe-only subscriber task per session-class hook.
///
/// A hook that subscribes only to agent-class events is skipped: those events
/// ride a process-wide channel outside the bus and are handled by the agent
/// runner. The returned handles complete when the bus channels close.
pub fn spawn_hook_runner<P>(
    bus: &Bus,
    hooks: &[ResolvedHook],
    session_cwd: &Path,
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
            // Subscribe before spawning so no event published after this call
            // is missed.
            let turn_rx = needs_turn(&hook).then(|| bus.turn().subscribe());
            let ui_rx = needs_ui(&hook).then(|| bus.ui_event().subscribe());
            let session_rx = Some(bus.session().subscribe());
            tokio::spawn(async move {
                run_hook(hook, cwd, process, turn_rx, ui_rx, session_rx).await;
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
/// subscribes to no agent-class event is skipped.
pub fn spawn_agent_hook_runner<P>(
    agent_tx: &AgentTx,
    hooks: &[ResolvedHook],
    session_cwd: &Path,
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
            // Subscribe before spawning so no event published after this call
            // is missed.
            let agent_rx = Some(agent_tx.subscribe());
            tokio::spawn(async move {
                run_agent_hook(hook, cwd, process, agent_rx).await;
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

/// Whether `hook` subscribes to a tool event.
fn needs_ui(hook: &ResolvedHook) -> bool {
    hook.events
        .iter()
        .any(|event| matches!(event, HookEvent::ToolStarted | HookEvent::ToolCompleted))
}

/// The serialized subscriber loop of one hook.
async fn run_hook<P>(
    hook: ResolvedHook,
    cwd: PathBuf,
    process: P,
    mut turn_rx: Option<TurnRx>,
    mut ui_rx: Option<UiEventRx>,
    mut session_rx: Option<SessionRx>,
) where
    P: HookProcess,
{
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
                Some(SessionEvent::Ended { session_id: id }) => {
                    if hook.events.contains(&HookEvent::SessionEnded) {
                        invocation = Some((
                            "session_ended",
                            serde_json::json!({"session_id": id}),
                        ));
                    }
                }
                Some(SessionEvent::Switched { to_session_id, .. }) => {
                    session_id = to_session_id;
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
                Some(TurnEvent::Completed { tool_calls }) => {
                    if hook.events.contains(&HookEvent::TurnCompleted) {
                        invocation = Some((
                            "turn_completed",
                            serde_json::json!({"tool_calls": tool_calls}),
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
                Some(_) => {}
                None => ui_rx = None,
            },
        }

        if let Some((event, payload)) = invocation {
            let envelope = build_envelope(event, &session_id, payload);
            if let Err(error) = process
                .run(&hook.script, event, &envelope, &cwd, hook.timeout)
                .await
            {
                log::warn!("{}", failure_warning(&hook.name, event, &error));
            }
        }

        if turn_rx.is_none() && ui_rx.is_none() && session_rx.is_none() {
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
            let envelope = build_envelope(event, "", payload);
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
