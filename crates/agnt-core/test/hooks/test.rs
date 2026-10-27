//! Unit tests for the user hook runner.
//!
//! The suite is split into topical sub-files. Shared imports, the `Result`
//! alias, and the mock process live here; each sub-file pulls them in with
//! `use super::*;`.

// region:    --- Modules

#[path = "envelope.rs"]
mod envelope;

#[path = "runner.rs"]
mod runner;

#[path = "runner_agent.rs"]
mod runner_agent;

#[path = "runner_session/agent_switched.rs"]
mod runner_session_agent_switched;

#[path = "runner_session/compaction.rs"]
mod runner_session_compaction;

#[path = "runner_session/model_switched.rs"]
mod runner_session_model_switched;

#[path = "runner_session/observe_only.rs"]
mod runner_session_observe_only;

#[path = "runner_session/permission_auto_resolved.rs"]
mod runner_session_permission_auto_resolved;

#[path = "runner_session/permission_decision_ignored.rs"]
mod runner_session_permission_decision_ignored;

#[path = "runner_session/permission_decision_submitted.rs"]
mod runner_session_permission_decision_submitted;

#[path = "runner_session/permission_decision_timed_out.rs"]
mod runner_session_permission_decision_timed_out;

#[path = "runner_session/permission_request_cancelled.rs"]
mod runner_session_permission_request_cancelled;

#[path = "runner_session/permission_requested.rs"]
mod runner_session_permission_requested;

#[path = "runner_session/session_lifecycle.rs"]
mod runner_session_session_lifecycle;

#[path = "runner_session/stamping.rs"]
mod runner_session_stamping;

// endregion: --- Modules

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::bus::{AgentEvent, AgentTx, Bus, CompactionEvent, SessionEvent, TurnEvent, create_bus};
use crate::config::{HookEvent, ResolvedHook};
use crate::hooks::{
    HookAgentContext, HookProcess, HookProcessError, build_envelope, spawn_agent_hook_runner,
    spawn_hook_runner,
};
use crate::protocol::event::{PermissionDecision, PermissionRequestContext, ToolDisplay, UiEvent};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- MockProcess

/// One recorded hook invocation.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Call {
    pub(super) script: PathBuf,
    pub(super) event: String,
    pub(super) envelope: String,
    pub(super) cwd: PathBuf,
    pub(super) timeout: Duration,
}

/// How the mock answers an invocation.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) enum Behaviour {
    #[default]
    Succeed,
    Fail,
    Timeout,
}

/// Records every invocation and answers per [`Behaviour`].
#[derive(Clone, Default)]
pub(super) struct MockProcess {
    calls: Arc<Mutex<Vec<Call>>>,
    behaviour: Behaviour,
}

impl MockProcess {
    pub(super) fn failing() -> Self {
        Self {
            behaviour: Behaviour::Fail,
            ..Self::default()
        }
    }

    pub(super) fn timing_out() -> Self {
        Self {
            behaviour: Behaviour::Timeout,
            ..Self::default()
        }
    }

    pub(super) fn calls(&self) -> Vec<Call> {
        self.calls
            .lock()
            .map(|calls| calls.clone())
            .unwrap_or_default()
    }
}

impl HookProcess for MockProcess {
    async fn run(
        &self,
        script: &Path,
        event: &str,
        envelope: &str,
        cwd: &Path,
        timeout: Duration,
    ) -> core::result::Result<(), HookProcessError> {
        if let Ok(mut calls) = self.calls.lock() {
            calls.push(Call {
                script: script.to_path_buf(),
                event: event.to_string(),
                envelope: envelope.to_string(),
                cwd: cwd.to_path_buf(),
                timeout,
            });
        }
        match self.behaviour {
            Behaviour::Succeed => Ok(()),
            Behaviour::Fail => Err(HookProcessError::Exit {
                exit_code: 1,
                stderr: "boom".to_string(),
            }),
            Behaviour::Timeout => Err(HookProcessError::Timeout(timeout)),
        }
    }
}

// endregion: --- MockProcess

// region:    --- Test Support

/// The session cwd every runner test passes.
pub(super) fn session_cwd() -> PathBuf {
    PathBuf::from("/tmp/hook-session")
}

/// The agent snapshot every runner test stamps its envelopes with.
pub(super) fn agent_context() -> HookAgentContext {
    HookAgentContext::default()
        .with_persona(Some("reviewer".to_string()))
        .with_model("openai/gpt-4o".to_string())
}

/// A resolved hook named `name` subscribed to `events`.
pub(super) fn hook(name: &str, events: &[HookEvent]) -> ResolvedHook {
    ResolvedHook {
        name: name.to_string(),
        events: events.to_vec(),
        script: PathBuf::from(format!("/hooks/{name}.nu")),
        timeout: Duration::from_millis(10_000),
    }
}

/// Await every spawned hook task.
pub(super) async fn finish(handles: Vec<tokio::task::JoinHandle<()>>) -> Result<()> {
    for handle in handles {
        handle
            .await
            .map_err(|e| format!("a hook task must not panic: {e}"))?;
    }
    Ok(())
}

/// The envelope of `call`, parsed as JSON.
pub(super) fn envelope_json(call: &Call) -> Result<serde_json::Value> {
    serde_json::from_str(&call.envelope).map_err(|e| -> Box<dyn std::error::Error> {
        format!("the envelope must be JSON: {e}").into()
    })
}

/// The `payload` object of `call`'s envelope.
pub(super) fn payload(call: &Call) -> Result<serde_json::Value> {
    let envelope = envelope_json(call)?;
    envelope
        .get("payload")
        .cloned()
        .ok_or_else(|| -> Box<dyn std::error::Error> { "the envelope must carry a payload".into() })
}

/// Spawn the session-class runner for `hooks` against `bus` with `process`.
pub(super) fn spawn(
    bus: &Bus,
    hooks: &[ResolvedHook],
    process: &MockProcess,
) -> Vec<tokio::task::JoinHandle<()>> {
    spawn_hook_runner(
        bus,
        hooks,
        &session_cwd(),
        &agent_context(),
        process.clone(),
    )
}

/// Spawn the agent-class runner for `hooks` against `agent_tx` with `process`.
pub(super) fn spawn_agent(
    agent_tx: &AgentTx,
    hooks: &[ResolvedHook],
    process: &MockProcess,
) -> Vec<tokio::task::JoinHandle<()>> {
    spawn_agent_hook_runner(
        agent_tx,
        hooks,
        &session_cwd(),
        &agent_context(),
        process.clone(),
    )
}

// endregion: --- Test Support
