//! Stderr (TTY) execution mode for the `agent` CLI.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use agnt_core::bus::CancelEvent;
use agnt_core::conversation::runtime::AgentConversationRuntime;
use agnt_core::orchestrator::run_single_turn;
use agnt_core::protocol::event::UiEvent;
use agnt_tty::policy::UiPolicy;
use agnt_tty::{StderrUiFactory, UiRendererFactory};
use nu_protocol::{Span, Value};

use crate::error::{Error, Result};

use super::{A2aContext, apply_a2a_turn_result, attach_a2a_context_session};

/// Run a single turn in stderr mode.
///
/// A pending A2A completion event or incoming task takes priority over the
/// CLI-provided `prompt`. When neither is present, `prompt` must be `Some`.
pub async fn run_stderr_mode(
    runtime_impl: &mut AgentConversationRuntime,
    prompt: Option<String>,
    context: Option<String>,
    ui_policy: UiPolicy,
    stderr_is_tty: bool,
    mut a2a: A2aContext,
) -> Result<Value> {
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Build the stderr renderer. In stderr mode core emits domain events on the
    // bus; this function subscribes to the bus channels and forwards each event
    // to the renderer while the turn runs.
    let stderr_renderer = StderrUiFactory::new(std::io::stderr(), stderr_is_tty).create(ui_policy);

    // Check for A2A completion events first (highest priority).
    if let Some(rx) = &mut a2a.completion_rx
        && let Ok(event) = rx.try_recv()
    {
        let prompt = event.to_prompt();
        return run_stderr_turn(runtime_impl, stderr_renderer, &cancel_flag, prompt, None).await;
    }

    // Then check for pending A2A incoming tasks.
    if let Some(rx) = &mut a2a.task_rx
        && let Ok(mut incoming) = rx.try_recv()
    {
        let task_id = incoming.task_id.clone();
        // An absent `contextId` means "start a fresh session" (A2A spec §3.4.1).
        // Mint a UUID so the client receives a context handle it can resume with.
        let context_id = agnt_a2a::resolve_context_id(incoming.context_id.take());
        incoming.context_id = Some(context_id.clone());
        attach_a2a_context_session(runtime_impl, &context_id).await;
        let prompt = incoming.to_prompt();
        let result =
            run_stderr_turn(runtime_impl, stderr_renderer, &cancel_flag, prompt, None).await;

        // Auto-complete or auto-fail the A2A task after the LLM finishes its
        // tool chain. On error the task must not stay in Working.
        if let Some(store) = &a2a.task_store {
            apply_a2a_turn_result(store, &task_id, &result);
        }

        return result;
    }

    let Some(prompt) = prompt else {
        return Err(Error::Custom(
            "No prompt provided for stderr mode".to_string(),
        ));
    };
    run_stderr_turn(runtime_impl, stderr_renderer, &cancel_flag, prompt, context).await
}

/// Run a single turn in stderr mode, forwarding bus lifecycle events to the
/// renderer and mapping SIGINT/cancel to a `CancelEvent` on the bus.
///
/// Core no longer threads a `ProgressUi` through the turn; it publishes domain
/// events on the shared `Bus`. This helper subscribes to `ui_event` and
/// `compaction`, emits each event on the stderr renderer while the turn runs,
/// and bridges the user's Ctrl+C (via `cancel_flag`) into a `CancelEvent` on
/// `bus.cancel()`.
async fn run_stderr_turn<R: agnt_core::renderer::UiRenderer + Send + 'static>(
    runtime_impl: &mut AgentConversationRuntime,
    renderer: R,
    cancel_flag: &Arc<AtomicBool>,
    prompt: String,
    context: Option<String>,
) -> Result<Value> {
    use std::sync::atomic::Ordering as AtomicOrdering;

    let bus = runtime_impl.bus.clone();
    // The renderer is shared between the drain task and the final flush.
    let shared = Arc::new(std::sync::Mutex::new(renderer));
    let stop = Arc::new(AtomicBool::new(false));

    // Spawn a task that drains the SIGINT-set cancel flag into the bus cancel
    // channel while the turn runs.
    let signal_flag = Arc::clone(cancel_flag);
    let cancel_bus = bus.clone();
    runtime_impl.spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            if signal_flag.swap(false, AtomicOrdering::SeqCst) {
                let _ = cancel_bus.cancel().send(CancelEvent::Requested).await;
            }
        }
    });

    // Drain task: subscribe to the bus channels the turn publishes on and forward
    // each event to the shared renderer. UI events arrive as `UiEvent` already;
    // compaction events convert at the boundary. Runs concurrently with the turn
    // so events stream in real time.
    let drain_shared = Arc::clone(&shared);
    let drain_stop = Arc::clone(&stop);
    let drain_bus = bus.clone();
    runtime_impl.spawn(async move {
        let mut ui_rx = drain_bus.ui_event().subscribe();
        let mut compaction_rx = drain_bus.compaction().subscribe();
        // Emit a periodic Tick so the stderr spinner animates while the turn
        // runs (the renderer's SystemTickGate throttles it).
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(16));
        loop {
            if drain_stop.load(AtomicOrdering::SeqCst) {
                break;
            }
            tokio::select! {
                Ok(event) = ui_rx.recv() => {
                    if let Ok(mut guard) = drain_shared.lock() {
                        guard.emit(&event);
                    }
                }
                ok = compaction_rx.recv() => {
                    if let Ok(event) = ok
                        && let Some(ui) = Option::<UiEvent>::from(event)
                        && let Ok(mut guard) = drain_shared.lock()
                    {
                        guard.emit(&ui);
                    }
                }
                _ = tick.tick() => {
                    if let Ok(mut guard) = drain_shared.lock() {
                        guard.emit(&UiEvent::Tick);
                    }
                }
            }
        }
    });

    // Drive the turn. Core publishes events on the bus; the drain task forwards
    // them to the renderer as they arrive.
    let result = run_single_turn(runtime_impl, &bus, prompt, context, Span::unknown())
        .await
        .map_err(Error::from);

    // Give the drain task a moment to process the final events before we stop it.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    // Stop the drain task and flush any remaining events.
    stop.store(true, AtomicOrdering::SeqCst);
    if let Ok(mut guard) = shared.lock() {
        guard.flush();
    }

    result
}
