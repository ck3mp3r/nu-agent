//! TUI and stderr execution modes for the `agent` CLI.
//!
//! These functions drive a built `AgentConversationRuntime` through either the
//! interactive TUI or the stderr renderer. They are the CLI counterpart of the
//! plugin's `mode_execute` module: the plugin feeds them a Nushell `Value`, the
//! CLI feeds them a plain prompt string.

// region:    --- Modules

mod stderr;
mod tui;

pub use stderr::run_stderr_mode;
pub use tui::run_tui_mode;

// endregion: --- Modules

use std::path::Path;
use std::sync::Arc;

use agnt_a2a::{
    A2aCompletionEvent, AgentCard, InMemoryTaskStore, IncomingTask, PeerCache, PeerDiscoveryImpl,
};
use agnt_core::bus::{AgentEvent, AgentTx, Bus, TurnEvent};
use agnt_core::config::{PluginConfig, ResolvedHook};
use agnt_core::conversation::runtime::AgentConversationRuntime;
use agnt_core::hooks::{
    HOOK_DRAIN_TIMEOUT, HookAgentContext, HookProcess, NuProcess, spawn_agent_hook_runner,
    spawn_hook_runner,
};
use agnt_core::protocol::contracts::UiMessageSnapshot;
use agnt_core::protocol::session_management::SessionPersistence;
use agnt_core::utils::value_ext::extract_response_text_from_value;
use nu_protocol::Value;
use tokio::task::JoinHandle;

use crate::error::Result;

// region:    --- Types

/// The process-wide agent lifecycle of one mode run.
///
/// Agent events are per-process, not per-session: one `Started`/`Shutdown` pair
/// per process lifetime, published on a standalone channel created at mode
/// entry. The agent-class hook runner is spawned here, once per process, and
/// skipped entirely when no hook subscribes to an agent-class event.
pub struct AgentLifecycle {
    tx: AgentTx,
    handles: Vec<JoinHandle<()>>,
}

/// Hydration context for TUI transcript replay on session attach.
pub struct TuiHydrationInput {
    pub should_hydrate: bool,
    pub initial_messages: Vec<UiMessageSnapshot>,
    pub last_total_tokens: Option<u64>,
}

/// A2A channels and handles threaded into the interactive modes.
///
/// The CLI default mode never starts an A2A server, so every field is `None`
/// there; the `Default` impl supplies that empty context.
#[derive(Default)]
pub struct A2aContext {
    pub task_rx: Option<tokio::sync::mpsc::Receiver<IncomingTask>>,
    pub completion_rx: Option<tokio::sync::mpsc::Receiver<A2aCompletionEvent>>,
    pub task_cancel_rx: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
    pub task_store: Option<Arc<InMemoryTaskStore>>,
    /// Handle to the server's mutable AgentCard, for updating on agent switch.
    pub card_handle: Option<Arc<std::sync::RwLock<AgentCard>>>,
    /// Shared peer cache, for updating the self-entry on agent switch.
    pub cache: Option<Arc<PeerCache>>,
    /// The port the A2A server is listening on.
    pub self_port: Option<u16>,
    /// Handle to the discovery implementation, for re-registering mDNS on agent switch.
    pub discovery: Option<Arc<std::sync::Mutex<PeerDiscoveryImpl>>>,
    /// The mesh key for mDNS discovery isolation.
    pub mesh_key: Option<String>,
}

/// Extra TUI-mode options bundled to keep `run_tui_mode` under the clippy
/// argument-count limit.
pub struct TuiModeOptions {
    pub a2a: A2aContext,
    pub theme_name: agnt_tui::rendering::theme::ThemeName,
}

// endregion: --- Types

// region:    --- Functions

/// Start the session-class hook runner for the configured hooks.
///
/// Returns the spawned tasks, empty when no hooks are configured — the zero-hook
/// case costs nothing and touches no file. A hook config that fails to resolve
/// is logged and skipped: hooks are observe-only, so a bad entry never blocks
/// the turn. `context` is the agent snapshot every envelope is stamped with.
pub fn spawn_configured_hooks<P>(
    config: Option<&PluginConfig>,
    bus: &Bus,
    cwd: &Path,
    context: &HookAgentContext,
    process: P,
) -> Vec<JoinHandle<()>>
where
    P: HookProcess + Send + Sync + 'static + Clone,
{
    let hooks = resolve_hooks_or_empty(config);
    if hooks.is_empty() {
        return Vec::new();
    }
    log::debug!("Starting {} hook runner(s)", hooks.len());
    spawn_hook_runner(bus, &hooks, cwd, context, process)
}

/// Resolve the configured hooks, or an empty list when the config is absent or
/// fails to resolve.
///
/// Hooks are observe-only, so a bad entry never blocks the turn: the failure is
/// logged and the caller proceeds with no hooks.
fn resolve_hooks_or_empty(config: Option<&PluginConfig>) -> Vec<ResolvedHook> {
    let Some(config) = config else {
        return Vec::new();
    };
    if config.hooks.is_empty() {
        return Vec::new();
    }
    match agnt_core::config::resolve_configured_hooks(config) {
        Ok(hooks) => hooks,
        Err(e) => {
            log::warn!("Failed to resolve hooks: {e}");
            Vec::new()
        }
    }
}

/// The shutdown reason for a mode result: `ok_reason` on success, `"error"`
/// otherwise.
pub fn shutdown_reason<T, E>(
    result: &core::result::Result<T, E>,
    ok_reason: &'static str,
) -> &'static str {
    if result.is_ok() { ok_reason } else { "error" }
}

/// Run `body` between the process-wide `AgentEvent::Started`/`Shutdown` pair.
///
/// The pair brackets every exit path of `body` — including an early return from
/// a setup failure — so a hook never observes Started alone. `ok_reason` is the
/// shutdown reason on success; a failed body always reports `"error"`.
pub async fn run_with_agent_lifecycle<T, F, Fut>(
    agent_lifecycle: AgentLifecycle,
    ok_reason: &'static str,
    body: F,
) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: core::future::Future<Output = Result<T>>,
{
    let result = body().await;
    agent_lifecycle
        .shutdown(shutdown_reason(&result, ok_reason))
        .await;
    agent_lifecycle.drain().await;
    result
}

/// Start the configured hooks with the production [`NuProcess`].
pub fn spawn_configured_hooks_with_nu(
    config: Option<&PluginConfig>,
    bus: &Bus,
    cwd: &Path,
    context: &HookAgentContext,
) -> Vec<JoinHandle<()>> {
    spawn_configured_hooks(config, bus, cwd, context, NuProcess)
}

/// Auto-complete an A2A task based on the LLM execution result.
///
/// Extracts the final text from the LLM response (handles both
/// `Value::String` and `Value::Record` with a `"response"` field)
/// and calls `store.complete_task()`. Logs a warning if the
/// completion call fails.
pub fn auto_complete_a2a_task(store: &InMemoryTaskStore, task_id: &str, response: &Value) {
    let final_text = extract_response_text_from_value(response);
    if let Err(e) = store.complete_task(task_id, &final_text) {
        log::warn!("Failed to auto-complete A2A task {task_id}: {e}");
    }
}

/// Auto-fail an A2A task with the turn's error message.
///
/// Mirrors [`auto_complete_a2a_task`]: transitions the task to `Failed` with
/// `error` as the status message. Logs a warning if the transition fails.
pub fn auto_fail_a2a_task(store: &InMemoryTaskStore, task_id: &str, error: &str) {
    if let Err(e) = store.fail_task(task_id, error) {
        log::warn!("Failed to auto-fail A2A task {task_id}: {e}");
    }
}

/// Apply a turn-outcome event to the A2A task store.
///
/// `TaskCompleted` completes the task with the turn output; `TaskFailed` fails
/// it with the error text. Every other variant is ignored. Store errors are
/// logged, never propagated.
pub fn apply_turn_event_to_store(store: &InMemoryTaskStore, event: &TurnEvent) {
    match event {
        TurnEvent::TaskCompleted { output, task_id } => {
            if let Err(e) = store.complete_task(task_id, output) {
                log::warn!("auto-complete failed for task {task_id}: {e}");
            }
        }
        TurnEvent::TaskFailed { task_id, error } => {
            if let Err(e) = store.fail_task(task_id, error) {
                log::warn!("auto-fail failed for task {task_id}: {e}");
            }
        }
        _ => {}
    }
}

/// Apply an A2A turn result to the task store.
///
/// A successful turn completes the task; a failed turn fails it with the
/// error message. Mirrors the TUI listener's outcome handling for stderr mode.
pub fn apply_a2a_turn_result(store: &InMemoryTaskStore, task_id: &str, result: &Result<Value>) {
    match result {
        Ok(response) => auto_complete_a2a_task(store, task_id, response),
        Err(error) => auto_fail_a2a_task(store, task_id, &error.to_string()),
    }
}

/// Attach the session derived from an A2A `contextId`.
///
/// Derives the session-store key as `{prefix}-{context_id}` (where `prefix` is
/// the path-hash prefix of the runtime's working directory) and attaches that
/// session, creating it when it does not exist yet. Logs a warning when the
/// attach fails; the turn still runs against the current session.
pub async fn attach_a2a_context_session(
    runtime_impl: &mut AgentConversationRuntime,
    context_id: &str,
) {
    let prefix = agnt_core::session::prefix::dir_prefix(runtime_impl.cwd());
    let key = agnt_a2a::derive_session_key(&prefix, context_id);
    log::info!("A2A context {context_id} -> session {key}");
    if let Err(e) = runtime_impl.attach_session(&key).await {
        log::warn!("Failed to attach A2A context session '{key}': {e}");
    }
}

// endregion: --- Functions

// region:    --- AgentLifecycle

impl AgentLifecycle {
    /// Publish `AgentEvent::Started` and spawn the agent-class hook runner.
    ///
    /// Takes ownership of `agent_tx` so [`drain`](Self::drain) can close the
    /// channel and let the runners finish.
    pub async fn start<P>(
        agent_tx: AgentTx,
        mode: &str,
        hooks: &[ResolvedHook],
        cwd: &Path,
        context: &HookAgentContext,
        process: P,
    ) -> Self
    where
        P: HookProcess + Send + Sync + 'static + Clone,
    {
        let handles = spawn_agent_hook_runner(&agent_tx, hooks, cwd, context, process);
        log::debug!("Starting {} agent hook runner(s)", handles.len());
        let _ = agent_tx
            .send(AgentEvent::Started {
                mode: mode.to_string(),
            })
            .await;
        Self {
            tx: agent_tx,
            handles,
        }
    }

    /// Publish `AgentEvent::Started` for `mode` with the configured hooks.
    pub async fn start_configured(
        mode: &str,
        config: Option<&PluginConfig>,
        cwd: &Path,
        context: &HookAgentContext,
    ) -> Self {
        let hooks = resolve_hooks_or_empty(config);
        Self::start(
            AgentTx::new("agent", 16),
            mode,
            &hooks,
            cwd,
            context,
            NuProcess,
        )
        .await
    }

    /// Publish `AgentEvent::Shutdown` with `reason`.
    pub async fn shutdown(&self, reason: &str) {
        let _ = self
            .tx
            .send(AgentEvent::Shutdown {
                reason: reason.to_string(),
            })
            .await;
    }

    /// Close the agent channel and wait for the spawned runners to finish.
    ///
    /// Returns the number of spawned runners, not the number that completed:
    /// a runner still busy when its wait expires is abandoned. The wait is
    /// [`HOOK_DRAIN_TIMEOUT`] per runner, applied sequentially, so N runners
    /// can delay exit by up to N times that value.
    pub async fn drain(self) -> usize {
        let drained = self.handles.len();
        drop(self.tx);
        for handle in self.handles {
            let _ = tokio::time::timeout(HOOK_DRAIN_TIMEOUT, handle).await;
        }
        drained
    }
}

// endregion: --- AgentLifecycle

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/mode/mod.rs"]
mod mode_test;

// endregion: --- Tests
