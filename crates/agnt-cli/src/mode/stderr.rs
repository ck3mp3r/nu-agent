//! Stderr (TTY) execution mode for the `agent` CLI.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use agnt_runtime::bus::CancelEvent;
use agnt_runtime::conversation::contracts::RuntimeAccess;
use agnt_runtime::orchestrator::run_single_turn;
use agnt_runtime::protocol::contracts::CoreRuntime;
use agnt_runtime::session::management::SessionPersistence;
use agnt_tty::policy::UiPolicy;
use agnt_tty::renderer::RenderInput;
use agnt_tty::{StderrUiFactory, UiRendererFactory};
use nu_protocol::{Span, Value};

use crate::error::{Error, Result};

use super::{
    A2aContext, AgentLifecycle, HOOK_DRAIN_TIMEOUT, TaskTerminal, apply_turn_terminal,
    attach_a2a_context_session, run_with_agent_lifecycle, spawn_configured_hooks_with_nu,
};

/// Run a single turn in stderr mode.
///
/// A pending A2A completion event or incoming task takes priority over the
/// CLI-provided `prompt`. When neither is present, `prompt` must be `Some`.
///
/// Publishes the process-wide `AgentEvent::Started`/`Shutdown` pair around the
/// turn and starts the user hook runners before it. Both drains are
/// best-effort: the hook tasks are detached and the process exits as soon as
/// this returns, so an in-flight hook subprocess gets a bounded moment to
/// finish.
pub async fn run_stderr_mode<R>(
    runtime_impl: &mut R,
    prompt: Option<String>,
    context: Option<String>,
    ui_policy: UiPolicy,
    stderr_is_tty: bool,
    a2a: A2aContext,
) -> Result<Value>
where
    R: CoreRuntime + RuntimeAccess + SessionPersistence,
{
    let config = runtime_impl.startup_plugin_config();
    let cwd = runtime_impl
        .mcp_caller_cwd()
        .unwrap_or(std::path::Path::new("."));

    // Zero hooks spawns nothing.
    let hook_bus = runtime_impl.bus_handle();
    let hook_handles = spawn_configured_hooks_with_nu(
        config,
        agnt_runtime::hooks::HookSources {
            stream: hook_bus.stream(),
            lifecycle: hook_bus.lifecycle(),
        },
        cwd,
        runtime_impl.paths().config_dir(),
        runtime_impl.hook_context(),
    );
    let agent_lifecycle = AgentLifecycle::start_configured(
        "stderr",
        config,
        cwd,
        runtime_impl.paths().config_dir(),
        runtime_impl.hook_context(),
    )
    .await;

    let result = run_with_agent_lifecycle(agent_lifecycle, "completed", || {
        run_stderr_mode_inner(runtime_impl, prompt, context, ui_policy, stderr_is_tty, a2a)
    })
    .await;

    // Best-effort hook drain: the turn's events are already published, so a
    // short pause lets a running hook subprocess complete before the process
    // exits. Gated on the handles so the zero-hook path pays nothing.
    if !hook_handles.is_empty() {
        tokio::time::sleep(HOOK_DRAIN_TIMEOUT).await;
    }

    result
}

/// The stderr-mode body, without the hook runner setup and drain.
async fn run_stderr_mode_inner<R>(
    runtime_impl: &mut R,
    prompt: Option<String>,
    context: Option<String>,
    ui_policy: UiPolicy,
    stderr_is_tty: bool,
    mut a2a: A2aContext,
) -> Result<Value>
where
    R: CoreRuntime + RuntimeAccess + SessionPersistence,
{
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Build the stderr renderer. In stderr mode core emits domain events on the
    // bus; this function subscribes to the bus channels and forwards each event
    // to the renderer while the turn runs.
    let stderr_renderer = StderrUiFactory::new(std::io::stderr(), stderr_is_tty).create(ui_policy);

    // Move the raw A2A channels onto the runtime vocabulary. The call is
    // idempotent, so a context already bridged by the dispatch layer is left
    // as-is. One scheduler turn lets the forwarders move a task the server
    // already queued onto the bridged channels before the drain below.
    a2a.bridge_a2a_channels();
    tokio::task::yield_now().await;

    // Check for external completion events first (highest priority).
    if let Some(rx) = &mut a2a.external_completion_rx
        && let Ok(completion) = rx.try_recv()
    {
        return run_stderr_turn(
            runtime_impl,
            stderr_renderer,
            &cancel_flag,
            None,
            completion.prompt,
            None,
        )
        .await;
    }

    // Then check for pending external tasks. The bridge already resolved the
    // context id and built the prompt from it.
    if let Some(rx) = &mut a2a.external_task_rx
        && let Ok(task) = rx.try_recv()
    {
        attach_a2a_context_session(runtime_impl, &task.context_id).await;
        let result = run_stderr_turn(
            runtime_impl,
            stderr_renderer,
            &cancel_flag,
            // The turn runs for this peer task, so its spine bracket carries
            // the id the caller holds in hand.
            Some(task.task_id.clone()),
            task.prompt,
            None,
        )
        .await;

        // Write back through the one turn→terminal mapping. stderr has no
        // spine listener, so the in-hand transition also publishes the row an
        // observer attributes; with no listener there is nothing to consume it
        // twice.
        if let Some(store) = &a2a.task_store {
            apply_turn_terminal(
                store,
                runtime_impl.bus_handle().lifecycle(),
                &task.task_id,
                TaskTerminal::from_result(&result),
            );
        }

        return result;
    }

    let Some(prompt) = prompt else {
        return Err(Error::Custom(
            "No prompt provided for stderr mode".to_string(),
        ));
    };
    run_stderr_turn(
        runtime_impl,
        stderr_renderer,
        &cancel_flag,
        None,
        prompt,
        context,
    )
    .await
}

/// Run a single turn in stderr mode, forwarding bus lifecycle events to the
/// renderer and mapping SIGINT/cancel to a `CancelEvent` on the bus.
///
/// `task_id` is the external task this turn runs for, or `None` for a prompt
/// the user submitted. It reaches the turn's spine bracket unchanged: the
/// attribution is the caller's, and this helper never decides one.
///
/// Core no longer threads a progress sink through the turn; it publishes domain
/// records on the shared `Bus`. This helper subscribes to the stream, narration,
/// and lifecycle concerns, draws each datum on the stderr renderer while the
/// turn runs, and bridges the user's Ctrl+C (via `cancel_flag`) into a
/// `CancelEvent` on `bus.cancel()`.
async fn run_stderr_turn<
    RU: CoreRuntime + RuntimeAccess,
    R: agnt_tty::renderer::UiRenderer + Send + 'static,
>(
    runtime_impl: &mut RU,
    renderer: R,
    cancel_flag: &Arc<AtomicBool>,
    task_id: Option<String>,
    prompt: String,
    context: Option<String>,
) -> Result<Value> {
    use std::sync::atomic::Ordering as AtomicOrdering;

    let bus = runtime_impl.bus_handle();
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

    // Drain task: subscribe to the concerns the turn publishes on and draw each
    // datum on the shared renderer. The stream carries the ordered records, the
    // narration tail the streamed text, and the lifecycle spine the turn steps.
    // Runs concurrently with the turn so events stream in real time.
    let drain_shared = Arc::clone(&shared);
    let drain_stop = Arc::clone(&stop);
    let drain_bus = bus.clone();
    runtime_impl.spawn(async move {
        let mut stream_rx = drain_bus.stream().subscribe();
        let mut narration_rx = drain_bus.narration().subscribe();
        let mut lifecycle_rx = drain_bus.lifecycle().subscribe();
        // Emit a periodic Tick so the stderr spinner animates while the turn
        // runs (the renderer's tick gate throttles it).
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(16));
        loop {
            if drain_stop.load(AtomicOrdering::SeqCst) {
                break;
            }
            tokio::select! {
                Ok(record) = stream_rx.recv() => {
                    if let Ok(mut guard) = drain_shared.lock() {
                        guard.draw(RenderInput::Record(&record));
                    }
                }
                Ok(event) = narration_rx.recv() => {
                    if let Ok(mut guard) = drain_shared.lock() {
                        guard.draw(RenderInput::Narration(&event));
                    }
                }
                Ok(event) = lifecycle_rx.recv() => {
                    if let Ok(mut guard) = drain_shared.lock() {
                        guard.draw(RenderInput::Lifecycle(&event));
                    }
                }
                _ = tick.tick() => {
                    if let Ok(mut guard) = drain_shared.lock() {
                        guard.tick();
                    }
                }
            }
        }
    });

    // Drive the turn. Core publishes events on the bus; the drain task forwards
    // them to the renderer as they arrive.
    let result = run_single_turn(runtime_impl, task_id, prompt, context, Span::unknown())
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
