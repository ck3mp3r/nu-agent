//! Interactive TUI execution mode for the `agent` CLI.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agnt_a2a::{Peer, mdns_name_for_switch, rebuild_card_for_switch, skill_from_persona};
use agnt_core::bus::OneshotTx;
use agnt_core::conversation::runtime::AgentConversationRuntime;
use agnt_core::orchestrator::{
    InteractiveLoopConfig, OnAgentSwitch, run_hydrated_interactive_loop_with_external_prompts,
    run_interactive_loop_with_external_prompts, run_single_turn,
};
use agnt_core::protocol::event::PermissionDecision;
use agnt_core::protocol::mcp_management::McpManagement;
use agnt_core::protocol::picker::{
    build_agent_picker_catalog, format_active_model_identity,
    model_picker_catalog_from_cached_startup_plugin_config,
};
use agnt_core::protocol::session_management::SessionPersistence;
use agnt_tty::policy::UiPolicy;
use agnt_tty::{StderrUiFactory, UiRendererFactory};
use agnt_tui::TuiInteractiveUi;
use agnt_tui::platform::terminal::TerminalLifecycle;
use agnt_tui::runtime::{
    AnsiTerminalBackend, HybridTerminalEvents, TtyTerminalEvents, TuiRuntimeRenderer,
    open_tty_reader,
};
use agnt_tui::state::ActivePicker;
use nu_protocol::{Span, Value};

use crate::error::{Error, Result};

use super::{
    AgentLifecycle, TuiHydrationInput, TuiModeOptions, apply_turn_event_to_store,
    run_with_agent_lifecycle, spawn_configured_hooks_with_nu,
};

/// Run the interactive TUI, or a single turn when `prompt` is `Some`.
///
/// `prompt` is `None` when the CLI was started without a prompt: the
/// interactive loop then reads prompts from the TUI input pane. When `prompt`
/// is `Some`, the function runs exactly one turn with the optional `context`.
pub async fn run_tui_mode(
    runtime_impl: AgentConversationRuntime,
    prompt: Option<String>,
    context: Option<String>,
    ui_policy: UiPolicy,
    hydration: TuiHydrationInput,
    options: TuiModeOptions,
) -> Result<Value> {
    // Agent events are process-scoped: publish Started before the first turn
    // runs and spawn the agent-class hook runner once for this process.
    let agent_lifecycle = AgentLifecycle::start_configured(
        "tui",
        runtime_impl.startup_plugin_config(),
        runtime_impl
            .mcp_caller_cwd()
            .unwrap_or(std::path::Path::new(".")),
        runtime_impl.paths.config_dir(),
        &runtime_impl.hook_context,
    )
    .await;

    run_with_agent_lifecycle(agent_lifecycle, "completed", || {
        run_tui_mode_inner(runtime_impl, prompt, context, ui_policy, hydration, options)
    })
    .await
}

/// The TUI body, without the process-wide agent lifecycle.
async fn run_tui_mode_inner(
    mut runtime_impl: AgentConversationRuntime,
    prompt: Option<String>,
    context: Option<String>,
    ui_policy: UiPolicy,
    hydration: TuiHydrationInput,
    options: TuiModeOptions,
) -> Result<Value> {
    // Set up the interactive permission pending map for TUI mode.
    // This Arc is shared between the worker thread (via InteractivePermissionResolver)
    // and the main thread (via the orchestrator's permission poll loop).
    let pending: Arc<Mutex<HashMap<String, OneshotTx<PermissionDecision>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    runtime_impl.interactive_pending = Some(Arc::clone(&pending));
    // The TUI renders pre-authorize previews and relies on the completion
    // display being suppressed to avoid showing the same diff twice.
    runtime_impl.renders_tool_previews = true;

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
    .map_err(|err| Error::Custom(format!("Failed to initialize TUI renderer: {err}")))?;

    let mut tui_ui = TuiInteractiveUi::new(runtime_renderer);
    tui_ui.set_theme(options.theme_name);
    // Create the persistence channel: the render loop forwards picker theme
    // selections over this channel; the receiving task saves them fire-and-forget.
    let (theme_persist_tx, mut theme_persist_rx) = tokio::sync::mpsc::channel::<String>(8);
    tui_ui.set_theme_persist_tx(theme_persist_tx);
    let theme_path = runtime_impl.paths.theme_path.clone();
    tokio::spawn(async move {
        while let Some(name) = theme_persist_rx.recv().await {
            // Normalize to the canonical kebab-case form so the preference file
            // always stores `{"theme": "catppuccin-<flavor>"}` regardless of
            // the upstream spelling (the picker emits Debug-format names).
            let Some(kebab) = agnt_tui::rendering::theme::ThemeName::from_name(&name)
                .map(|n| n.kebab_name().to_string())
            else {
                continue;
            };
            let pref = agnt_core::theme_pref::ThemePreference { theme: Some(kebab) };
            if let Err(e) = pref.save_to(&theme_path) {
                log::warn!("Failed to persist theme preference: {e}");
            }
        }
    });
    let active_model_identity =
        format_active_model_identity(runtime_impl.provider_name(), runtime_impl.model());
    tui_ui.set_active_model_identity(active_model_identity.clone());
    let model_picker_catalog = model_picker_catalog_from_cached_startup_plugin_config(
        runtime_impl.startup_plugin_config(),
        active_model_identity.as_str(),
    );
    tui_ui.set_picker_options(ActivePicker::Model, model_picker_catalog);
    let agent_picker_catalog = build_agent_picker_catalog(runtime_impl.available_agent_summaries());
    tui_ui.set_picker_options(ActivePicker::Agent, agent_picker_catalog);
    // Populate session picker from session store
    {
        let cwd = runtime_impl
            .mcp_caller_cwd()
            .unwrap_or(std::path::Path::new("."));
        let sessions = runtime_impl.list_sessions(cwd).await;
        match sessions {
            Ok(sessions) => {
                let options: Vec<agnt_tui::state::PickerOption> = sessions
                    .into_iter()
                    .map(agnt_tui::state::PickerOption::from)
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
            let skills = agnt_core::protocol::skills::discover_skill_catalog_for_cwd(&cwd);
            tui_ui.set_skills_projection(skills);
        }
        None => tui_ui.mark_skills_discovery_failed(),
    }
    tui_ui.set_mcp_lifecycle_projection(runtime_impl.mcp_lifecycle_projection().to_vec());
    tui_ui.set_llm_visible_mcp_tool_count(runtime_impl.llm_visible_mcp_tool_count());
    // The per-server count and name events are published before the render loop
    // subscribes to the UI-state channel, so they are dropped. Push them here
    // instead, after the projection: `set_mcp_servers` retains counts and names
    // only for servers the projection lists.
    for (server_name, names) in runtime_impl.llm_visible_mcp_tool_names_by_server() {
        tui_ui.set_mcp_visible_tool_count_by_server_name(&server_name, names.len());
        tui_ui.set_mcp_visible_tool_names_by_server_name(&server_name, names);
    }
    tui_ui.set_context_window_max_tokens(runtime_impl.max_context_tokens());

    tui_ui.push_startup_logo();

    // The incoming A2A task, completion-event, and cancel channels are all
    // `tokio::sync` receivers. They are passed directly into `InteractiveLoopConfig`,
    // and the orchestrator `select!`s over them. No std-bridge forwarder threads
    // are needed.
    let task_cancel_rx = options.a2a.task_cancel_rx;
    let a2a_task_rx = options.a2a.task_rx;
    let a2a_completion_rx = options.a2a.completion_rx;
    let a2a_task_store = options.a2a.task_store.clone();

    // Auto-complete or auto-fail A2A tasks from turn-outcome events on the
    // signal bus. The session stage publishes `TurnEvent::TaskCompleted` on
    // success and `TurnEvent::TaskFailed` on error/cancellation, each carrying
    // the task ID. A background task reads these and transitions the task.
    if let Some(store) = &options.a2a.task_store {
        let store = Arc::clone(store);
        let bus = runtime_impl.bus.clone();
        tokio::spawn(async move {
            let mut turn_rx = bus.turn().subscribe();
            loop {
                match turn_rx.recv().await {
                    Ok(event) => apply_turn_event_to_store(&store, &event),
                    Err(agnt_core::bus::ChannelError::Lagged { .. }) => {}
                    Err(agnt_core::bus::ChannelError::Closed) => break,
                    Err(_) => {}
                }
            }
        });
    }

    // Build the on_agent_switch callback for A2A card updates.
    // Always built (regardless of A2A status) so the persona icon can be
    // updated on agent switch even when A2A is disabled.
    let card_handle = options.a2a.card_handle;
    let cache = options.a2a.cache.clone();
    let self_port = options.a2a.self_port;
    let discovery = options.a2a.discovery.clone();
    let mesh_key = options.a2a.mesh_key.clone();
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

    // Start the user hook runner before the loop so the first prompt's events
    // are observed. Zero hooks spawns nothing.
    let _hook_handles = spawn_configured_hooks_with_nu(
        runtime_impl.startup_plugin_config(),
        &runtime_impl.bus,
        runtime_impl
            .mcp_caller_cwd()
            .unwrap_or(std::path::Path::new(".")),
        runtime_impl.paths.config_dir(),
        &runtime_impl.hook_context,
    );

    terminal_lifecycle
        .enter()
        .map_err(|e| Error::Custom(format!("Failed to enter terminal raw mode: {e}")))?;

    let result = if let Some(prompt) = prompt {
        // Single-turn TUI: core emits domain events on the bus; the renderer
        // subscribes to bus channels directly (the render loop subscribes), so we
        // pass the bus rather than a `ProgressUi`.
        let bus = runtime_impl.bus.clone();
        run_single_turn(&mut runtime_impl, &bus, prompt, context, Span::unknown())
            .await
            .map_err(Error::from)
    } else if hydration.should_hydrate {
        // The startup `HydrateTranscript` event is published before the render
        // loop subscribes to the UI-state channel, so it is dropped. Push the
        // persisted transcript directly, before `make_render_loop_spawner`
        // moves the coordinator into the render loop.
        tui_ui.hydrate_transcript(
            hydration.initial_messages.clone(),
            hydration.last_total_tokens,
        );
        let mut config = InteractiveLoopConfig::new(Span::unknown())
            .with_bus(runtime_impl.bus.clone())
            .with_hydration(hydration.initial_messages, hydration.last_total_tokens)
            .with_interactive_pending(Some(Arc::clone(&pending)))
            .with_task_cancel_rx(task_cancel_rx)
            .with_a2a_task_rx(a2a_task_rx)
            .with_a2a_completion_rx(a2a_completion_rx)
            .with_task_store(a2a_task_store.clone())
            .with_spawn_render_loop(tui_ui.make_render_loop_spawner(runtime_impl.bus.clone()));
        if let Some(cb) = on_agent_switch {
            config = config.with_on_agent_switch(cb);
        }
        run_hydrated_interactive_loop_with_external_prompts(runtime_impl, config)
            .await
            .map_err(Error::from)
    } else {
        let mut config = InteractiveLoopConfig::new(Span::unknown())
            .with_bus(runtime_impl.bus.clone())
            .with_interactive_pending(Some(Arc::clone(&pending)))
            .with_task_cancel_rx(task_cancel_rx)
            .with_a2a_task_rx(a2a_task_rx)
            .with_a2a_completion_rx(a2a_completion_rx)
            .with_task_store(a2a_task_store.clone())
            .with_spawn_render_loop(tui_ui.make_render_loop_spawner(runtime_impl.bus.clone()));
        if let Some(cb) = on_agent_switch {
            config = config.with_on_agent_switch(cb);
        }
        run_interactive_loop_with_external_prompts(runtime_impl, config)
            .await
            .map_err(Error::from)
    };

    let _ = terminal_lifecycle.restore();
    result
}
