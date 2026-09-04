use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nu_protocol::{LabeledError, Value};

use nu_agent_a2a::{
    A2aCompletionEvent, AgentCard, InMemoryTaskStore, IncomingTask, Part, Peer, PeerCache,
    PeerDiscoveryImpl, mdns_name_for_switch, rebuild_card_for_switch, skill_from_persona,
};
use nu_agent_core::bus::{OneshotTx, TurnEvent};
use nu_agent_core::utils::value_ext::extract_response_text_from_value;
use nu_agent_core::{
    conversation::runtime::AgentConversationRuntime,
    orchestrator::{
        InteractiveLoopConfig, OnAgentSwitch, run_hydrated_interactive_loop_with_external_prompts,
        run_interactive_loop_with_external_prompts, run_single_turn,
    },
    protocol::{
        contracts::UiMessageSnapshot, event::PermissionDecision, mcp_management::McpManagement,
        session_management::SessionPersistence,
    },
};
use nu_agent_tty::policy::UiPolicy;
use nu_agent_tty::{StderrUiFactory, UiRendererFactory};
use nu_agent_tui::TuiInteractiveUi;
use nu_agent_tui::platform::terminal::TerminalLifecycle;
use nu_agent_tui::runtime::{
    AnsiTerminalBackend, HybridTerminalEvents, TtyTerminalEvents, TuiRuntimeRenderer,
    open_tty_reader,
};
use nu_agent_tui::state::ActivePicker;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentMode {
    Tui,
    Stderr,
}

impl AgentMode {
    pub(crate) fn is_tui(self) -> bool {
        matches!(self, Self::Tui)
    }
}

/// Auto-complete an A2A task based on the LLM execution result.
///
/// Extracts the final text from the LLM response (handles both
/// `Value::String` and `Value::Record` with a `"response"` field)
/// and calls `store.complete_task()`.  Logs a warning if the
/// completion call fails.
pub(crate) fn auto_complete_a2a_task(store: &InMemoryTaskStore, task_id: &str, response: &Value) {
    let final_text = extract_response_text_from_value(response);
    if let Err(e) = store.complete_task(task_id, &final_text) {
        log::warn!("Failed to auto-complete A2A task {task_id}: {e}");
    }
}

/// Whether the plugin should call `enter_foreground()` to receive SIGINT.
/// True for TUI (always needs it) and for stderr mode when stderr is a TTY
/// (user has a terminal and may press Ctrl+C).
pub(crate) fn should_enter_foreground(mode: AgentMode, stderr_is_tty: bool) -> bool {
    mode.is_tui() || stderr_is_tty
}

pub(crate) fn resolve_agent_mode(
    input_is_nothing: bool,
    stdin_is_tty: bool,
    stderr_is_tty: bool,
) -> AgentMode {
    if input_is_nothing && stdin_is_tty && stderr_is_tty {
        AgentMode::Tui
    } else {
        AgentMode::Stderr
    }
}

/// Hydration context for TUI transcript replay on session attach.
pub(crate) struct TuiHydrationInput {
    pub should_hydrate: bool,
    pub initial_messages: Vec<UiMessageSnapshot>,
    pub last_total_tokens: Option<u64>,
}

pub(crate) struct A2aContext {
    pub(crate) task_rx: Option<tokio::sync::mpsc::Receiver<IncomingTask>>,
    pub(crate) completion_rx: Option<tokio::sync::mpsc::Receiver<A2aCompletionEvent>>,
    pub(crate) task_cancel_rx: Option<tokio::sync::mpsc::UnboundedReceiver<String>>,
    pub(crate) task_store: Option<Arc<InMemoryTaskStore>>,
    /// Handle to the server's mutable AgentCard, for updating on agent switch.
    pub(crate) card_handle: Option<Arc<std::sync::RwLock<AgentCard>>>,
    /// Shared peer cache, for updating the self-entry on agent switch.
    pub(crate) cache: Option<Arc<PeerCache>>,
    /// The port the A2A server is listening on.
    pub(crate) self_port: Option<u16>,
    /// Handle to the discovery implementation, for re-registering mDNS on agent switch.
    pub(crate) discovery: Option<Arc<std::sync::Mutex<PeerDiscoveryImpl>>>,
    /// The mesh key for mDNS discovery isolation.
    pub(crate) mesh_key: Option<String>,
}

pub(crate) async fn run_tui_mode(
    mut runtime_impl: AgentConversationRuntime,
    input: &Value,
    input_is_nothing: bool,
    span: nu_protocol::Span,
    ui_policy: UiPolicy,
    hydration: TuiHydrationInput,
    a2a: A2aContext,
) -> Result<Value, LabeledError> {
    // Set up the interactive permission pending map for TUI mode.
    // This Arc is shared between the worker thread (via InteractivePermissionResolver)
    // and the main thread (via the orchestrator's permission poll loop).
    let pending: Arc<Mutex<HashMap<String, OneshotTx<PermissionDecision>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    runtime_impl.interactive_pending = Some(Arc::clone(&pending));

    let mut terminal_lifecycle =
        TerminalLifecycle::new(AnsiTerminalBackend::new(std::io::stderr()));

    let (columns, rows) = crossterm::terminal::size().unwrap_or((120, 30));
    let fallback_events = open_tty_reader()
        .ok()
        .and_then(|tty_reader| TtyTerminalEvents::new(tty_reader, Duration::from_millis(30)).ok());

    let runtime_renderer = TuiRuntimeRenderer::new_live(
        StderrUiFactory::new(std::io::stderr(), false).create(ui_policy),
        HybridTerminalEvents::new(Duration::from_millis(60), fallback_events),
        columns,
        rows,
    )
    .map_err(|err| LabeledError::new(format!("Failed to initialize TUI renderer: {err}")))?;

    let mut tui_ui = TuiInteractiveUi::new(runtime_renderer);
    let active_model_identity = super::picker::format_active_model_identity(
        runtime_impl.provider_name(),
        runtime_impl.model(),
    );
    tui_ui.set_active_model_identity(active_model_identity.clone());
    let model_picker_catalog =
        super::picker::model_picker_catalog_from_cached_startup_plugin_config(
            runtime_impl.startup_plugin_config(),
            active_model_identity.as_str(),
        );
    tui_ui.set_picker_options(ActivePicker::Model, model_picker_catalog);
    let agent_picker_catalog = super::picker::build_agent_picker_catalog(
        runtime_impl.available_agent_summaries(),
        runtime_impl.agent_identity(),
    );
    tui_ui.set_picker_options(ActivePicker::Agent, agent_picker_catalog);
    // Populate session picker from session store
    {
        let cwd = runtime_impl
            .mcp_caller_cwd()
            .unwrap_or(std::path::Path::new("."));
        let sessions = runtime_impl.list_sessions(cwd).await;
        match sessions {
            Ok(sessions) => {
                let options: Vec<nu_agent_tui::state::PickerOption> = sessions
                    .into_iter()
                    .map(|info| {
                        let title = info.title.clone();
                        let display = title.clone().unwrap_or_else(|| "(untitled)".to_string());
                        nu_agent_tui::state::PickerOption {
                            id: info.id.clone(),
                            display: display.clone(),
                            search_text: display.clone(),
                            payload: nu_agent_tui::state::PickerPayload::Session {
                                session_id: info.id,
                                title,
                                created_at: info.last_active,
                            },
                        }
                    })
                    .collect();
                tui_ui.set_picker_options(ActivePicker::Session, options);
            }
            Err(e) => {
                log::warn!("Failed to list sessions for picker: {e}");
            }
        }
    }
    let cycle_names: Vec<String> = runtime_impl
        .available_agent_summaries()
        .iter()
        .map(|s| s.name.clone())
        .collect();
    tui_ui.set_agent_cycle_names(cycle_names);
    if let Some(identity) = runtime_impl.agent_identity() {
        tui_ui.set_active_agent_identity(identity);
    }
    tui_ui.set_active_persona_icon(runtime_impl.agent_icon().map(|s| s.to_string()));
    let caller_cwd = runtime_impl.mcp_caller_cwd().map(|p| p.to_path_buf());
    tui_ui.set_repo_branch_caller_cwd(caller_cwd.clone());
    match caller_cwd {
        Some(cwd) => {
            let skills = nu_agent_core::protocol::skills::discover_skill_catalog_for_cwd(&cwd);
            tui_ui.set_skills_projection(skills);
        }
        None => tui_ui.mark_skills_discovery_failed(),
    }
    tui_ui.set_mcp_lifecycle_projection(runtime_impl.mcp_lifecycle_projection().to_vec());
    tui_ui.set_llm_visible_mcp_tool_count(runtime_impl.llm_visible_mcp_tool_count());
    tui_ui.set_context_window_max_tokens(runtime_impl.max_context_tokens());

    if !hydration.should_hydrate {
        tui_ui.push_startup_logo();
    }

    // The incoming A2A task, completion-event, and cancel channels are all
    // `tokio::sync` receivers. They are passed directly into `InteractiveLoopConfig`,
    // and the orchestrator `select!`s over them. No std-bridge forwarder threads
    // are needed.
    let task_cancel_rx = a2a.task_cancel_rx;
    let a2a_task_rx = a2a.task_rx;
    let a2a_completion_rx = a2a.completion_rx;

    // Auto-complete A2A tasks from turn-completion events on the signal bus.
    // The session stage publishes `TurnEvent::TaskCompleted` with the task ID after
    // each external turn completes. A background thread reads these and calls
    // `store.complete_task()`.
    if let Some(store) = &a2a.task_store {
        let store = Arc::clone(store);
        let bus = runtime_impl.bus.clone();
        tokio::spawn(async move {
            let mut turn_rx = bus.turn().subscribe();
            loop {
                match turn_rx.recv().await {
                    Ok(TurnEvent::TaskCompleted { output, task_id }) => {
                        if let Err(e) = store.complete_task(&task_id, &output) {
                            log::warn!("auto-complete failed for task {task_id}: {e}");
                        }
                    }
                    Ok(TurnEvent::Completed { .. }) => {}
                    Ok(TurnEvent::Started { .. }) => {}
                    Err(nu_agent_core::bus::ChannelError::Lagged { .. }) => {}
                    Err(nu_agent_core::bus::ChannelError::Closed) => break,
                    Err(_) => {}
                }
            }
        });
    }

    // Build the on_agent_switch callback for A2A card updates.
    // Always built (regardless of A2A status) so the persona icon can be
    // updated on agent switch even when A2A is disabled.
    let card_handle = a2a.card_handle;
    let cache = a2a.cache.clone();
    let self_port = a2a.self_port;
    let discovery = a2a.discovery.clone();
    let mesh_key = a2a.mesh_key.clone();
    let on_agent_switch: Option<OnAgentSwitch> = Some(Arc::new(
        move |name: String, description: Option<String>, _icon: Option<String>| {
            let Some(card_handle) = card_handle.as_ref() else {
                // No A2A card to update; nothing further to do here.
                return;
            };
            let mut card = card_handle.write().expect("agent_card lock");
            let old_name = card.name.clone();
            let skill = skill_from_persona(&name, description.as_deref());
            let new_card =
                rebuild_card_for_switch(&card, &name, description.as_deref(), vec![skill]);
            *card = new_card.clone();
            // Update the peer cache self-entry so agent_list reflects the new name.
            if let (Some(cache), Some(port)) = (cache.clone(), self_port) {
                cache.remove(&old_name);
                cache.add_or_update(Peer {
                    name: name.clone(),
                    url: card.url.clone(),
                    host: "127.0.0.1".to_string(),
                    port,
                    card: Some(card.clone()),
                    discovered_at: std::time::Instant::now(),
                });
            }
            // Re-register mDNS with the new name so other agents discover the change.
            if let (Some(discovery), Some(mesh_key)) = (discovery.as_ref(), mesh_key.as_ref()) {
                let mut d = discovery.lock().expect("discovery lock");
                let old_fullname = d.fullname().map(|s| s.to_string());
                if let Some(old_fullname) = &old_fullname
                    && let Some(port) = self_port
                {
                    let new_mdns_name = mdns_name_for_switch(&old_name, &name, port);
                    d.rename(old_fullname, &new_mdns_name, port, &card, mesh_key);
                }
            }
        },
    ) as OnAgentSwitch);

    terminal_lifecycle
        .enter()
        .map_err(|e| LabeledError::new(format!("Failed to enter terminal raw mode: {e}")))?;

    let result = if input_is_nothing {
        if hydration.should_hydrate {
            let mut config = InteractiveLoopConfig::new(span)
                .with_bus(runtime_impl.bus.clone())
                .with_hydration(hydration.initial_messages, hydration.last_total_tokens)
                .with_interactive_pending(Some(Arc::clone(&pending)))
                .with_task_cancel_rx(task_cancel_rx)
                .with_a2a_task_rx(a2a_task_rx)
                .with_a2a_completion_rx(a2a_completion_rx)
                .with_spawn_render_loop(tui_ui.make_render_loop_spawner(runtime_impl.bus.clone()));
            if let Some(cb) = on_agent_switch {
                config = config.with_on_agent_switch(cb);
            }
            run_hydrated_interactive_loop_with_external_prompts(runtime_impl, config).await
        } else {
            let mut config = InteractiveLoopConfig::new(span)
                .with_bus(runtime_impl.bus.clone())
                .with_interactive_pending(Some(Arc::clone(&pending)))
                .with_task_cancel_rx(task_cancel_rx)
                .with_a2a_task_rx(a2a_task_rx)
                .with_a2a_completion_rx(a2a_completion_rx)
                .with_spawn_render_loop(tui_ui.make_render_loop_spawner(runtime_impl.bus.clone()));
            if let Some(cb) = on_agent_switch {
                config = config.with_on_agent_switch(cb);
            }
            run_interactive_loop_with_external_prompts(runtime_impl, config).await
        }
    } else {
        let (prompt, context) = super::input::extract_prompt_and_context(input)?;
        // Single-turn TUI: core emits domain events on the bus; the renderer
        // subscribes to bus channels directly (the render loop subscribes), so we
        // pass the bus rather than a `ProgressUi`.
        let bus = runtime_impl.bus.clone();
        run_single_turn(&mut runtime_impl, &bus, prompt, context, span).await
    };

    let _ = terminal_lifecycle.restore();
    result
}

pub(crate) async fn run_stderr_mode(
    runtime_impl: &mut AgentConversationRuntime,
    input: &Value,
    span: nu_protocol::Span,
    ui_policy: UiPolicy,
    stderr_is_tty: bool,
    mut a2a: A2aContext,
) -> Result<Value, LabeledError> {
    let cancel_flag = Arc::new(AtomicBool::new(false));

    // Build the stderr renderer. In stderr mode core emits domain events on the
    // bus; this function subscribes to the bus channels and forwards each event
    // to the renderer while the turn runs.
    let stderr_renderer = StderrUiFactory::new(std::io::stderr(), stderr_is_tty).create(ui_policy);

    // Check for A2A completion events first (highest priority).
    if let Some(rx) = &mut a2a.completion_rx
        && let Ok(event) = rx.try_recv()
    {
        let prompt = format!(
            "[A2A Task {} completed by {}]: {}\n\nStatus: {}.",
            event.task_id, event.agent_name, event.result, event.status
        );
        return run_stderr_turn(
            runtime_impl,
            stderr_renderer,
            &cancel_flag,
            prompt,
            None,
            span,
        )
        .await;
    }

    // Then check for pending A2A incoming tasks.
    if let Some(rx) = &mut a2a.task_rx
        && let Ok(incoming) = rx.try_recv()
    {
        let task_id = incoming.task_id.clone();
        let text: String = incoming
            .message
            .parts
            .iter()
            .filter_map(|p| match p {
                Part::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join(" ");
        let prompt = format!(
            "[A2A Task {} from {}]: {}\n\nProcess this request and respond with your answer. Your response will be automatically delivered as the task result.",
            incoming.task_id, incoming.sender_url, text
        );
        let result = run_stderr_turn(
            runtime_impl,
            stderr_renderer,
            &cancel_flag,
            prompt,
            None,
            span,
        )
        .await;

        // Auto-complete the A2A task after the LLM finishes its tool chain
        if let Some(store) = &a2a.task_store
            && let Ok(response) = &result
        {
            auto_complete_a2a_task(store, &task_id, response);
        }

        return result;
    }

    let (prompt, context) = super::input::extract_prompt_and_context(input)?;
    run_stderr_turn(
        runtime_impl,
        stderr_renderer,
        &cancel_flag,
        prompt,
        context,
        span,
    )
    .await
}

/// Run a single turn in stderr mode, forwarding bus lifecycle events to the
/// renderer and mapping SIGINT/cancel to a `CancelEvent` on the bus.
///
/// Core no longer threads a `ProgressUi` through the turn; it publishes domain
/// events (tool/LLM/turn/warning/compaction/permission) on the shared `Bus`.
/// This helper subscribes to those channels, converts each event to a `UiEvent`
/// at the boundary, and emits it on the stderr renderer while the turn runs.
/// It also bridges the user's Ctrl+C (via `cancel_flag`) into a `CancelEvent`
/// on `bus.cancel()`.
async fn run_stderr_turn<R: nu_agent_core::renderer::UiRenderer + Send + 'static>(
    runtime_impl: &mut AgentConversationRuntime,
    renderer: R,
    cancel_flag: &Arc<AtomicBool>,
    prompt: String,
    context: Option<String>,
    span: nu_protocol::Span,
) -> Result<Value, LabeledError> {
    use std::sync::atomic::Ordering as AtomicOrdering;

    use nu_agent_core::protocol::event::UiEvent;

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
                let _ = cancel_bus
                    .cancel()
                    .send(nu_agent_core::bus::CancelEvent::Requested)
                    .await;
            }
        }
    });

    // Drain task: subscribe to the bus channels the turn publishes on and forward
    // each event to the shared renderer (converted to `UiEvent` at the boundary).
    // Runs concurrently with the turn so events stream in real time.
    let drain_shared = Arc::clone(&shared);
    let drain_stop = Arc::clone(&stop);
    let drain_bus = bus.clone();
    runtime_impl.spawn(async move {
        let mut tool_rx = drain_bus.tool().subscribe();
        let mut llm_rx = drain_bus.llm().subscribe();
        let mut turn_rx = drain_bus.turn().subscribe();
        let mut warning_rx = drain_bus.warning().subscribe();
        let mut compaction_rx = drain_bus.compaction().subscribe();
        let mut permission_rx = drain_bus.permission().subscribe();
        // Emit a periodic Tick so the stderr spinner animates while the turn
        // runs (the renderer's SystemTickGate throttles it).
        let mut tick_interval = tokio::time::interval(std::time::Duration::from_millis(16));
        loop {
            if drain_stop.load(AtomicOrdering::SeqCst) {
                break;
            }
            // Drain all channels each iteration; emit converted events. Each
            // channel is handled explicitly because the receiver types differ.
            drain_channel(&mut tool_rx, &drain_shared);
            drain_channel(&mut llm_rx, &drain_shared);
            drain_channel(&mut turn_rx, &drain_shared);
            drain_channel(&mut warning_rx, &drain_shared);
            drain_channel(&mut compaction_rx, &drain_shared);
            drain_channel(&mut permission_rx, &drain_shared);
            // Tick the spinner on a fixed cadence.
            tick_interval.tick().await;
            if let Ok(mut guard) = drain_shared.lock() {
                guard.emit(&UiEvent::Tick);
            }
        }
    });

    // Drive the turn. Core publishes events on the bus; the drain task forwards
    // them to the renderer as they arrive.
    let result = run_single_turn(runtime_impl, &bus, prompt, context, span).await;

    // Give the drain task a moment to process the final events before we stop it.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;

    // Stop the drain task and flush any remaining events.
    stop.store(true, AtomicOrdering::SeqCst);
    if let Ok(mut guard) = shared.lock() {
        guard.flush();
    }

    result
}

/// Drain a single broadcast channel, forwarding each event that converts to a
/// `Some(UiEvent)` to the shared stderr renderer. `Empty` ends the drain;
/// `Lagged` skips to the next event; `Closed` ends the drain.
fn drain_channel<T, R>(
    rx: &mut nu_agent_core::bus::BroadcastRx<T>,
    renderer: &Arc<std::sync::Mutex<R>>,
) where
    Option<nu_agent_core::protocol::event::UiEvent>: From<T>,
    T: Clone + Send + 'static,
    R: nu_agent_core::renderer::UiRenderer,
{
    loop {
        match rx.try_recv() {
            Ok(event) => {
                if let Some(ui_event) =
                    Option::<nu_agent_core::protocol::event::UiEvent>::from(event)
                    && let Ok(mut guard) = renderer.lock()
                {
                    guard.emit(&ui_event);
                }
            }
            Err(nu_agent_core::bus::TryRecvError::Empty) => break,
            Err(nu_agent_core::bus::TryRecvError::Lagged(_)) => continue,
            Err(nu_agent_core::bus::TryRecvError::Closed) => break,
        }
    }
}
