//! Interactive TUI execution mode for the `agent` CLI.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, RwLock, RwLockWriteGuard};

use agnt_a2a::{
    AgentCard, Peer, PeerCache, PeerDiscoveryImpl, mdns_name_for_switch, rebuild_card_for_switch,
    skill_from_persona,
};
use agnt_runtime::bus::OneshotTx;
use agnt_runtime::catalog::PickerCatalogSource;
use agnt_runtime::conversation::contracts::{DecisionSource, RuntimeAccess};
use agnt_runtime::orchestrator::{AgentSwitchHook, LoopConfig, run};
use agnt_runtime::protocol::McpManagement;
use agnt_runtime::protocol::contracts::CoreRuntime;
use agnt_runtime::protocol::event::PermissionDecision;
use agnt_runtime::protocol::model_switching::ModelSwitching;
use agnt_runtime::protocol::picker::{
    build_agent_picker_catalog, format_active_model_identity,
    model_picker_catalog_from_cached_startup_plugin_config,
};
use agnt_runtime::session::management::{SessionPersistence, SessionState};
use agnt_tty::policy::UiPolicy;
use agnt_tui::TuiInteractiveUi;
use agnt_tui::platform::terminal::TerminalLifecycle;
use agnt_tui::runtime::{AnsiTerminalBackend, TuiRuntimeRenderer};
use agnt_tui::state::ActivePicker;
use nu_protocol::Value;

use crate::error::{Error, Result};

use super::{
    AgentLifecycle, TuiHydrationInput, TuiModeOptions, apply_turn_event_to_store,
    run_with_agent_lifecycle, spawn_configured_hooks_with_nu,
};

// region:    --- Types

/// Keeps the A2A agent card, the peer-cache self entry, and the mDNS
/// registration in sync after an agent switch.
#[derive(Clone)]
struct TuiAgentSwitchHook {
    card_handle: Option<Arc<RwLock<AgentCard>>>,
    cache: Option<Arc<PeerCache>>,
    self_port: Option<u16>,
    discovery: Option<Arc<Mutex<PeerDiscoveryImpl>>>,
    mesh_key: Option<String>,
}

impl AgentSwitchHook for TuiAgentSwitchHook {
    fn on_agent_switch(&self, name: String, description: Option<String>, _icon: Option<String>) {
        let Some(card_handle) = self.card_handle.as_ref() else {
            // No A2A card to update; nothing further to do here.
            return;
        };
        let mut card = lock_card(card_handle);
        let old_name = card.name.clone();
        let skill = skill_from_persona(&name, description.as_deref());
        let new_card = rebuild_card_for_switch(&card, &name, description.as_deref(), vec![skill]);
        *card = new_card.clone();
        // Update the peer cache self-entry so agent_list reflects the new name.
        if let (Some(cache), Some(port)) = (self.cache.as_ref(), self.self_port) {
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
        if let (Some(discovery), Some(mesh_key)) = (self.discovery.as_ref(), self.mesh_key.as_ref())
        {
            let mut d = lock_discovery(discovery);
            let old_fullname = d.fullname().map(|s| s.to_string());
            if let Some(old_fullname) = &old_fullname
                && let Some(port) = self.self_port
            {
                let new_mdns_name = mdns_name_for_switch(&old_name, &name, port);
                d.rename(old_fullname, &new_mdns_name, port, &card, mesh_key);
            }
        }
    }
}

// endregion: --- Types

/// Run the interactive TUI.
///
/// The interactive loop reads prompts from the TUI input pane.
pub async fn run_tui_mode<T>(
    runtime_impl: T,
    prompt: Option<String>,
    context: Option<String>,
    ui_policy: UiPolicy,
    hydration: TuiHydrationInput,
    options: TuiModeOptions,
) -> Result<Value>
where
    T: CoreRuntime
        + McpManagement
        + ModelSwitching
        + SessionPersistence
        + SessionState
        + DecisionSource
        + RuntimeAccess
        + PickerCatalogSource
        + Send
        + 'static,
{
    // Agent events are process-scoped: publish Started before the first turn
    // runs and spawn the agent-class hook runner once for this process.
    let agent_lifecycle = AgentLifecycle::start_configured(
        "tui",
        runtime_impl.startup_plugin_config(),
        runtime_impl
            .mcp_caller_cwd()
            .unwrap_or(std::path::Path::new(".")),
        runtime_impl.paths().config_dir(),
        runtime_impl.hook_context(),
    )
    .await;

    run_with_agent_lifecycle(agent_lifecycle, "completed", || {
        run_tui_mode_inner(runtime_impl, prompt, context, ui_policy, hydration, options)
    })
    .await
}

/// The TUI body, without the process-wide agent lifecycle.
async fn run_tui_mode_inner<T>(
    mut runtime_impl: T,
    _prompt: Option<String>,
    _context: Option<String>,
    _ui_policy: UiPolicy,
    hydration: TuiHydrationInput,
    options: TuiModeOptions,
) -> Result<Value>
where
    T: CoreRuntime
        + McpManagement
        + ModelSwitching
        + SessionPersistence
        + SessionState
        + DecisionSource
        + RuntimeAccess
        + PickerCatalogSource
        + Send
        + 'static,
{
    // Set up the interactive permission pending map for TUI mode.
    // This Arc is shared between the worker thread (via InteractivePermissionResolver)
    // and the main thread (via the orchestrator's permission poll loop).
    let pending: Arc<Mutex<HashMap<String, OneshotTx<PermissionDecision>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    runtime_impl = runtime_impl.with_decision_source(Some(Arc::clone(&pending)));

    let mut terminal_lifecycle =
        TerminalLifecycle::new(AnsiTerminalBackend::new(std::io::stderr()));

    let (columns, rows) = crossterm::terminal::size().unwrap_or((120, 30));

    let runtime_renderer = TuiRuntimeRenderer::new_live(columns, rows)
        .map_err(|err| Error::Custom(format!("Failed to initialize TUI renderer: {err}")))?;

    let mut tui_ui = TuiInteractiveUi::new(runtime_renderer);
    tui_ui.set_theme(options.theme_name);
    // Create the persistence channel: the render loop forwards picker theme
    // selections over this channel; the receiving task saves them fire-and-forget.
    let (theme_persist_tx, mut theme_persist_rx) = tokio::sync::mpsc::channel::<String>(8);
    tui_ui.set_theme_persist_tx(theme_persist_tx);
    let theme_path = runtime_impl.paths().theme_path.clone();
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
            let pref = agnt_runtime::theme_pref::ThemePreference { theme: Some(kebab) };
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
    tui_ui.set_active_persona_icon(RuntimeAccess::agent_icon(&runtime_impl).map(|s| s.to_string()));
    let caller_cwd = runtime_impl.mcp_caller_cwd().map(|p| p.to_path_buf());
    tui_ui.set_repo_branch_caller_cwd(caller_cwd.clone());
    match caller_cwd {
        Some(cwd) => {
            let skills = agnt_runtime::protocol::skills::discover_skill_catalog_for_cwd(&cwd);
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

    // The bridged external-task and completion channels are `tokio::sync`
    // receivers. They are passed directly into `LoopConfig`, and the
    // orchestrator `select!`s over them. The task-cancel channel stays raw: it
    // carries only IDs, so nothing needs converting.
    let task_cancel_rx = options.a2a.task_cancel_rx;
    let external_task_rx = options.a2a.external_task_rx;
    let external_completion_rx = options.a2a.external_completion_rx;
    let external_task_store = options.a2a.gate.clone();

    // Auto-complete or auto-fail A2A tasks from turn-outcome events on the
    // lifecycle concern. The session stage publishes `LifecycleEvent::TaskCompleted`
    // on success and `LifecycleEvent::TaskFailed` on error/cancellation, each
    // carrying the task ID. A background task reads these and transitions the task.
    if let Some(store) = &options.a2a.task_store {
        let store = Arc::clone(store);
        let bus = runtime_impl.bus_handle();
        tokio::spawn(async move {
            let mut turn_rx = bus.lifecycle().subscribe();
            loop {
                match turn_rx.recv().await {
                    Ok(event) => apply_turn_event_to_store(&store, &event),
                    Err(agnt_runtime::bus::ReceiveError::Closed) => break,
                    Err(_) => {}
                }
            }
        });
    }

    // Build the on_agent_switch hook for A2A card updates.
    // Always built (regardless of A2A status) so the persona icon can be
    // updated on agent switch even when A2A is disabled.
    let on_agent_switch = TuiAgentSwitchHook {
        card_handle: options.a2a.card_handle,
        cache: options.a2a.cache.clone(),
        self_port: options.a2a.self_port,
        discovery: options.a2a.discovery.clone(),
        mesh_key: options.a2a.mesh_key.clone(),
    };

    // Start the user hook runner before the loop so the first prompt's events
    // are observed. Zero hooks spawns nothing.
    let hook_bus = runtime_impl.bus_handle();
    let _hook_handles = spawn_configured_hooks_with_nu(
        runtime_impl.startup_plugin_config(),
        agnt_runtime::hooks::HookSources {
            stream: hook_bus.stream(),
            lifecycle: hook_bus.lifecycle(),
        },
        runtime_impl
            .mcp_caller_cwd()
            .unwrap_or(std::path::Path::new(".")),
        runtime_impl.paths().config_dir(),
        runtime_impl.hook_context(),
    );

    terminal_lifecycle
        .enter()
        .map_err(|e| Error::Custom(format!("Failed to enter terminal raw mode: {e}")))?;

    let result = if hydration.should_hydrate {
        // The startup `HydrateTranscript` event is published before the render
        // loop subscribes to the UI-state channel, so it is dropped. Push the
        // persisted transcript directly, before `make_render_loop_spawner`
        // moves the coordinator into the render loop.
        tui_ui.hydrate_transcript(
            hydration.initial_messages.clone(),
            hydration.last_total_tokens,
        );
        let config = LoopConfig::default()
            .with_bus(runtime_impl.bus_handle())
            .with_hydration(hydration.initial_messages, hydration.last_total_tokens)
            .with_interactive_pending(Some(Arc::clone(&pending)))
            .with_task_cancel_rx(task_cancel_rx)
            .with_external_task_rx(external_task_rx)
            .with_external_completion_rx(external_completion_rx)
            .with_external_task_store(external_task_store.clone())
            .with_event_sink(tui_ui.make_render_loop_spawner(runtime_impl.bus_handle()))
            .with_on_agent_switch(on_agent_switch);
        run(runtime_impl, config).await.map_err(Error::from)
    } else {
        let config = LoopConfig::default()
            .with_bus(runtime_impl.bus_handle())
            .with_interactive_pending(Some(Arc::clone(&pending)))
            .with_task_cancel_rx(task_cancel_rx)
            .with_external_task_rx(external_task_rx)
            .with_external_completion_rx(external_completion_rx)
            .with_external_task_store(external_task_store.clone())
            .with_event_sink(tui_ui.make_render_loop_spawner(runtime_impl.bus_handle()))
            .with_on_agent_switch(on_agent_switch);
        run(runtime_impl, config).await.map_err(Error::from)
    };

    let _ = terminal_lifecycle.restore();
    result
}

// region:    --- Support

/// The server's live card, recovered if a writer panicked mid-swap.
fn lock_card(card_handle: &Arc<RwLock<AgentCard>>) -> RwLockWriteGuard<'_, AgentCard> {
    card_handle.write().unwrap_or_else(|e| e.into_inner())
}

/// The discovery backend, recovered if a caller panicked mid-restart.
fn lock_discovery(discovery: &Arc<Mutex<PeerDiscoveryImpl>>) -> MutexGuard<'_, PeerDiscoveryImpl> {
    discovery.lock().unwrap_or_else(|e| e.into_inner())
}

// endregion: --- Support
