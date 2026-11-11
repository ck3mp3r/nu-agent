use std::time::Instant;

use agnt_runtime::bus::{Control, Facts, Lifecycle, Narration, Stream};
use agnt_runtime::orchestrator::events::OrchestratorEvent;
use nu_protocol::LabeledError;
use tokio::sync::mpsc;

use crate::interaction::cancel::CancelController;
use crate::interaction::input::TerminalEvent;
use crate::runtime::map_crossterm_event;
use crate::runtime::render::current_time_millis;
use crate::runtime::{RuntimeCoordinator, TuiRuntimeRenderer};
use crate::state::{ActivePicker, PickerOption};

#[cfg(test)]
#[path = "../test/interactive.rs"]
mod interactive_test;

pub struct TuiInteractiveUi {
    renderer: TuiRuntimeRenderer,
    theme_persist_tx: Option<mpsc::Sender<String>>,
}

/// Auxiliary channels the render loop drains and publishes on, alongside
/// terminal events.
///
/// The render loop is a bus consumer: it folds the record stream, the narration
/// tail, the facts snapshots and the control intake. The picker catalogs are
/// the surface's own state and never ride a channel here — they come back on
/// the reply hand carried by the `UiRequest`.
pub(crate) struct RenderLoopChannels<'a> {
    pub(crate) branch_rx: mpsc::Receiver<()>,
    pub(crate) theme_persist_tx: Option<mpsc::Sender<String>>,
    pub(crate) stream: &'a Stream,
    pub(crate) narration: &'a Narration,
    pub(crate) control: &'a Control,
    pub(crate) lifecycle: &'a Lifecycle,
    pub(crate) facts: &'a Facts,
}

/// The render loop's only timer: the busy spinner's own cadence. A tick
/// services time-driven content alone — the status message's expiry and the
/// spinner's cadence step — and renders only when one of them changed, so an
/// unchanged frame emits zero bytes.
pub(crate) const RENDER_TICK: std::time::Duration =
    std::time::Duration::from_millis(crate::runtime::SPINNER_CADENCE_MS as u64);

impl TuiInteractiveUi {
    pub fn new(renderer: TuiRuntimeRenderer) -> Self {
        Self {
            renderer,
            theme_persist_tx: None,
        }
    }

    /// Set the channel the render loop uses to forward picker theme selections
    /// to the command layer for persistence. The TUI never writes the file.
    pub fn set_theme_persist_tx(&mut self, tx: mpsc::Sender<String>) {
        self.theme_persist_tx = Some(tx);
    }

    /// Set the active theme by name on the coordinator before the render loop
    /// starts. Mirrors the `SwitchRequest::Theme` arm.
    pub fn set_theme(&mut self, name: crate::rendering::theme::ThemeName) {
        self.renderer.coordinator.set_theme(name);
    }

    pub fn set_active_model_identity(&mut self, active_model_identity: String) {
        self.renderer
            .coordinator
            .state
            .status
            .identity
            .active_model_identity = active_model_identity;
    }

    pub fn set_active_persona_icon(&mut self, icon: Option<String>) {
        self.renderer
            .coordinator
            .state
            .status
            .identity
            .active_persona_icon = icon;
    }

    pub fn set_mcp_lifecycle_projection(
        &mut self,
        projection: Vec<agnt_runtime::tools::mcp::runtime::McpServerLifecycle>,
    ) {
        self.renderer.set_mcp_lifecycle_projection(projection);
    }

    pub fn set_skills_projection(
        &mut self,
        skills: Vec<agnt_runtime::protocol::skills::DiscoverableSkill>,
    ) {
        self.renderer.set_skills_projection(skills);
    }

    pub fn mark_skills_discovery_failed(&mut self) {
        self.renderer.mark_skills_discovery_failed();
    }

    pub fn set_llm_visible_mcp_tool_count(&mut self, count: usize) {
        self.renderer
            .coordinator
            .set_llm_visible_mcp_tool_count(count);
    }

    pub fn hydrate_transcript(
        &mut self,
        messages: Vec<agnt_runtime::protocol::contracts::UiMessageSnapshot>,
        last_total_tokens: Option<u64>,
    ) {
        self.renderer
            .coordinator
            .hydrate_transcript_from_messages(messages, last_total_tokens);
    }

    pub fn set_mcp_visible_tool_count_by_server_name(&mut self, server_name: &str, count: usize) {
        self.renderer
            .coordinator
            .set_mcp_visible_tool_count_by_server_name(server_name, count);
    }

    pub fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        server_name: &str,
        names: Vec<String>,
    ) {
        self.renderer
            .coordinator
            .set_mcp_visible_tool_names_by_server_name(server_name, names);
    }

    pub fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.renderer
            .coordinator
            .state
            .status
            .tokens
            .set_context_window_max_tokens(max_tokens);
    }

    pub fn set_picker_options<T: Into<PickerOption>>(
        &mut self,
        kind: ActivePicker,
        options: Vec<T>,
    ) {
        self.renderer
            .coordinator
            .state
            .set_picker_options(kind, options);
    }

    pub fn set_active_agent_identity(&mut self, name: &str) {
        self.renderer
            .coordinator
            .state
            .set_active_agent_identity(name);
    }

    /// Pop the next theme name the picker selected, for the command layer to
    /// persist. The TUI never writes the preference file itself.
    pub fn take_next_theme_persist_request(&mut self) -> Option<String> {
        self.renderer
            .coordinator
            .state
            .take_next_theme_persist_request()
    }

    pub fn set_agent_cycle_names(&mut self, names: Vec<String>) {
        self.renderer
            .coordinator
            .state
            .status
            .identity
            .agent_cycle_names = names;
    }

    pub fn set_repo_branch_caller_cwd(&mut self, caller_cwd: Option<std::path::PathBuf>) {
        self.renderer.set_repo_branch_caller_cwd(caller_cwd);
    }

    pub fn push_startup_logo(&mut self) {
        self.renderer.coordinator.state.push_startup_logo();
    }

    pub fn make_render_loop_spawner(
        &mut self,
        bus: agnt_runtime::bus::Bus,
    ) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let coordinator = std::mem::replace(
            &mut self.renderer.coordinator,
            RuntimeCoordinator::new(120, 30, None),
        );
        let theme_persist_tx = self.theme_persist_tx.take();
        let live_terminal = self.renderer.take_live_terminal();
        let cancel_controller = coordinator.cancel_controller.clone();
        // Subscribe a filesystem watcher to git ref files before the loop starts
        // so branch changes are delivered as events, not polled.
        let watch_targets = coordinator.repo_branch_watch_targets();
        let (branch_tx, branch_rx) = mpsc::channel(8);
        let branch_watcher = if watch_targets.is_empty() {
            None
        } else {
            match crate::runtime::branch_watcher::spawn_branch_watcher(watch_targets, branch_tx) {
                Ok(w) => Some(w),
                Err(e) => {
                    log::warn!("git branch watcher failed: {e}");
                    None
                }
            }
        };
        move |event_tx| {
            let (terminal_tx, terminal_rx) = mpsc::channel(64);
            if let Err(err) = spawn_terminal_input(terminal_tx) {
                log::warn!("{err}");
            }
            tokio::spawn(async move {
                let mut coordinator = coordinator;
                let mut live = live_terminal;
                let mut live_ref = live.as_mut().map(|l| &mut l.terminal);
                let _branch_watcher = branch_watcher;
                let channels = RenderLoopChannels {
                    branch_rx,
                    theme_persist_tx,
                    stream: bus.stream(),
                    narration: bus.narration(),
                    control: bus.control(),
                    lifecycle: bus.lifecycle(),
                    facts: bus.facts(),
                };
                run_render_loop(
                    &mut coordinator,
                    &cancel_controller,
                    event_tx,
                    terminal_rx,
                    &mut live_ref,
                    channels,
                )
                .await;
            });
        }
    }
}

/// Spawn a dedicated thread that reads crossterm terminal events and sends
/// them on the given channel.
///
/// The reader is a plain OS thread, not a `tokio::task::spawn_blocking` task.
/// It blocks in `crossterm::event::read()` until a key arrives, and dropping a
/// tokio runtime waits for every blocking task to finish. A pending read would
/// therefore stall process exit until the user pressed a key. A detached thread
/// is torn down with the process instead.
pub fn spawn_terminal_input(
    terminal_event_tx: mpsc::Sender<TerminalEvent>,
) -> Result<std::thread::JoinHandle<()>, String> {
    std::thread::Builder::new()
        .name("tui-terminal-input".to_string())
        .spawn(move || {
            loop {
                let event = match crossterm::event::read() {
                    Ok(event) => event,
                    Err(err) => {
                        log::warn!("crossterm read failed: {err}");
                        break;
                    }
                };
                if let Some(terminal_event) = map_crossterm_event(event)
                    && terminal_event_tx.blocking_send(terminal_event).is_err()
                {
                    break;
                }
            }
        })
        .map_err(|err| format!("failed to spawn terminal input thread: {err}"))
}

/// Forward any pending picker theme selections to the command layer for
/// persistence. The TUI never writes the preference file itself.
async fn drain_theme_persist(
    coordinator: &mut RuntimeCoordinator,
    theme_persist_tx: &Option<mpsc::Sender<String>>,
) {
    let Some(tx) = theme_persist_tx else {
        return;
    };
    while let Some(name) = coordinator.state.take_next_theme_persist_request() {
        if tx.send(name).await.is_err() {
            return;
        }
    }
}

/// Render loop driving `RuntimeCoordinator` from terminal events, bus
/// channels, and a periodic tick. Owns `&mut RuntimeCoordinator` and calls
/// `AppState` methods directly.
pub(crate) async fn run_render_loop<
    B: ratatui::backend::Backend + crate::runtime::backend::FrameSync,
>(
    coordinator: &mut RuntimeCoordinator,
    cancel_controller: &CancelController,
    event_tx: mpsc::Sender<OrchestratorEvent>,
    mut terminal_event_rx: mpsc::Receiver<TerminalEvent>,
    live_terminal: &mut Option<&mut ratatui::Terminal<B>>,
    mut channels: RenderLoopChannels<'_>,
) {
    let mut stream_rx = channels.stream.subscribe();
    let mut narration_rx = channels.narration.subscribe();
    let mut control_rx = channels.control.subscribe();
    let mut lifecycle_rx = channels.lifecycle.subscribe();
    let mut facts_rx = channels.facts.subscribe();
    let mut render_timer = tokio::time::interval(RENDER_TICK);

    loop {
        tokio::select! {
            maybe = terminal_event_rx.recv() => {
                let Some(event) = maybe else { return; };
                coordinator.handle_terminal_event(event);
                if flush_pending(coordinator, &event_tx, cancel_controller, &channels).await {
                    return;
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
            record = stream_rx.recv() => {
                // A lag re-syncs the subscription at the tail: the dropped
                // records are counted and gone, and nothing needs repair.
                if let Ok(record) = record {
                    // Coalesce the burst that queued behind the awaited record:
                    // one chunk fold per wake instead of one per chunk. A lag
                    // ends the drain — the buffered records arrive on later
                    // iterations, and a chunk carries `aggregated`, so a dropped
                    // older chunk loses no content.
                    let mut batch = vec![record];
                    while let Ok(next) = stream_rx.try_recv() {
                        batch.push(next);
                    }
                    // A turn completion (or failure) clears the active prompt.
                    // Drain any prompts stacked during the turn into
                    // PromptSubmitted events now, without waiting for the next
                    // terminal input.
                    if coordinator.reduce_record_batch(batch)
                        && flush_pending(coordinator, &event_tx, cancel_controller, &channels).await
                    {
                        return;
                    }
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
            event = narration_rx.recv() => {
                // Narration is the oldest-droppable tail: the surviving events
                // keep their receipt order, and a lagged subscription resumes at
                // the tail.
                if let Ok(event) = event {
                    let mut batch = vec![event];
                    while let Ok(next) = narration_rx.try_recv() {
                        batch.push(next);
                    }
                    if coordinator.reduce_narration_batch(batch)
                        && flush_pending(coordinator, &event_tx, cancel_controller, &channels).await
                    {
                        return;
                    }
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
            command = control_rx.recv() => {
                // Control carries commands addressed to whichever owner names
                // them: the surface folds the ones it owns, and the loop's own
                // intake never reaches the fold.
                if let Ok(command) = command
                    && coordinator.reduce_control(&command)
                    && flush_pending(coordinator, &event_tx, cancel_controller, &channels).await
                {
                    return;
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
            event = lifecycle_rx.recv() => {
                // The turn spine is the surface's clock over a replay batch: the
                // first live turn that starts after a switch ends the batch, and
                // the gate becomes the only card source again. A lag skips whole
                // spine steps; a skipped step is never guessed at.
                if let Ok(event) = event
                    && coordinator.reduce_lifecycle(&event)
                    && flush_pending(coordinator, &event_tx, cancel_controller, &channels).await
                {
                    return;
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
            fact = facts_rx.recv() => {
                // A lag skips snapshots, never a state: the subscription
                // resumes at the tail carrying each field's latest value, so a
                // lagged receive needs no repair of its own.
                if let Ok(fact) = fact
                    && coordinator.reduce_fact_record(fact)
                {
                    coordinator.mark_render_needed();
                    let _ = coordinator.render_if_needed(live_terminal);
                }
            }
            _ = channels.branch_rx.recv() => {
                coordinator.refresh_repo_branch();
                let _ = coordinator.render_if_needed(live_terminal);
            }
            _ = render_timer.tick() => {
                // The picker catalogs arrive on the surface's own reply hands,
                // outside the bus, so the tick is what drains them.
                coordinator.poll_picker_replies();
                if coordinator.has_pending_status_message()
                    && coordinator.expire_status_message_if_due(Instant::now())
                {
                    coordinator.mark_render_needed();
                }
                if coordinator.spinner_cadence_advanced(current_time_millis()) {
                    coordinator.mark_render_needed();
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
        }
    }
}

/// Drain the state's outbound effects: the prompts and commands the loop owns,
/// the theme preference the command layer persists, and the two hands that end
/// the loop (quit and fatal error).
///
/// Returns whether the render loop must stop.
async fn flush_pending(
    coordinator: &mut RuntimeCoordinator,
    event_tx: &mpsc::Sender<OrchestratorEvent>,
    cancel_controller: &CancelController,
    channels: &RenderLoopChannels<'_>,
) -> bool {
    let pending = coordinator.state.take_pending_events(cancel_controller);
    for ev in pending {
        if event_tx.send(ev).await.is_err() {
            return true;
        }
    }
    drain_theme_persist(coordinator, &channels.theme_persist_tx).await;
    if coordinator.state.quit_requested {
        let _ = event_tx.send(OrchestratorEvent::Quit).await;
        return true;
    }
    if let Some(msg) = coordinator.fatal_error() {
        let _ = event_tx
            .send(OrchestratorEvent::FatalError(LabeledError::new(
                msg.to_string(),
            )))
            .await;
        return true;
    }
    false
}
