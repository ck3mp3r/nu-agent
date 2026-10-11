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

use std::sync::Arc;

use agnt_a2a::{
    A2aCompletionEvent, AgentCard, InMemoryTaskStore, IncomingTask, PeerCache, PeerDiscoveryImpl,
};
use agnt_core::bus::TurnEvent;
use agnt_core::conversation::runtime::AgentConversationRuntime;
use agnt_core::protocol::contracts::UiMessageSnapshot;
use agnt_core::protocol::session_management::SessionPersistence;
use agnt_core::utils::value_ext::extract_response_text_from_value;
use nu_protocol::Value;

use crate::error::Result;

// region:    --- Types

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
