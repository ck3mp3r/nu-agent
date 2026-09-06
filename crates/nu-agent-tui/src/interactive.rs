use std::time::Instant;

use nu_agent_core::orchestrator::OrchestratorEvent;
use nu_agent_core::protocol::{
    contracts::{ProgressUi, UserInputUi},
    event::UiEvent,
};
use nu_agent_core::renderer::UiRenderer;
use nu_protocol::LabeledError;
use tokio::sync::mpsc;

use crate::interaction::cancel::CancelController;
use crate::interaction::input::TerminalEvent;
use crate::runtime::map_crossterm_event;
use crate::runtime::{HybridTerminalEvents, RuntimeCoordinator, TuiRuntimeRenderer};
use crate::state::{ActivePicker, PickerOption};

#[cfg(test)]
#[path = "interactive_test.rs"]
mod interactive_test;

pub struct TuiInteractiveUi<R>
where
    R: UiRenderer,
{
    renderer: TuiRuntimeRenderer<R, HybridTerminalEvents>,
}

impl<R> TuiInteractiveUi<R>
where
    R: UiRenderer,
{
    pub fn new(renderer: TuiRuntimeRenderer<R, HybridTerminalEvents>) -> Self {
        Self { renderer }
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
        projection: Vec<nu_agent_core::tools::mcp::runtime::McpServerLifecycle>,
    ) {
        self.renderer.set_mcp_lifecycle_projection(projection);
    }

    pub fn set_skills_projection(
        &mut self,
        skills: Vec<nu_agent_core::protocol::skills::DiscoverableSkill>,
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
        bus: nu_agent_core::bus::Bus,
    ) -> impl FnOnce(mpsc::Sender<OrchestratorEvent>) + Send + 'static {
        let coordinator = std::mem::replace(
            &mut self.renderer.coordinator,
            RuntimeCoordinator::new(120, 30, Some(true)),
        );
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
            spawn_terminal_input(terminal_tx);
            tokio::spawn(async move {
                let mut coordinator = coordinator;
                let mut live = live_terminal;
                let mut live_ref = live.as_mut().map(|l| &mut l.terminal);
                let _branch_watcher = branch_watcher;
                run_render_loop(
                    &mut coordinator,
                    &bus,
                    &cancel_controller,
                    event_tx,
                    terminal_rx,
                    &mut live_ref,
                    branch_rx,
                )
                .await;
            });
        }
    }
}

impl<R> ProgressUi for TuiInteractiveUi<R>
where
    R: UiRenderer,
{
    fn emit(&mut self, event: &UiEvent) {
        self.renderer.emit(event);
    }

    fn flush(&mut self) {
        self.renderer.flush();
    }

    fn take_cancel_requested(&self) -> bool {
        self.renderer.take_cancel_requested()
    }

    fn emit_batch(&mut self, events: &[UiEvent]) {
        self.renderer.emit_batch(events);
    }
}

impl<R> UserInputUi for TuiInteractiveUi<R>
where
    R: UiRenderer,
{
    fn event_sender(&self) -> &mpsc::Sender<OrchestratorEvent> {
        self.renderer.event_sender()
    }
}

/// Spawn a blocking task that reads crossterm terminal events and sends
/// them on the given channel.
pub fn spawn_terminal_input(
    terminal_event_tx: mpsc::Sender<TerminalEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
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
}

/// Render loop driving `RuntimeCoordinator` from terminal events, bus
/// channels, and a periodic tick. Owns `&mut RuntimeCoordinator` and calls
/// `AppState` methods directly.
pub(crate) async fn run_render_loop<B: ratatui::backend::Backend>(
    coordinator: &mut RuntimeCoordinator,
    bus: &nu_agent_core::bus::Bus,
    cancel_controller: &CancelController,
    event_tx: mpsc::Sender<OrchestratorEvent>,
    mut terminal_event_rx: mpsc::Receiver<TerminalEvent>,
    live_terminal: &mut Option<&mut ratatui::Terminal<B>>,
    mut branch_rx: mpsc::Receiver<()>,
) {
    let mut tool_rx = bus.tool().subscribe();
    let mut llm_rx = bus.llm().subscribe();
    let mut warning_rx = bus.warning().subscribe();
    let mut compaction_rx = bus.compaction().subscribe();
    let mut turn_rx = bus.turn().subscribe();
    let mut session_rx = bus.session().subscribe();
    let mut permission_rx = bus.permission().subscribe();
    let mut ui_state_rx = bus.ui_state().subscribe();
    let mut render_timer = tokio::time::interval(std::time::Duration::from_millis(80));

    loop {
        tokio::select! {
            maybe = terminal_event_rx.recv() => {
                let Some(event) = maybe else { return; };
                coordinator.handle_terminal_event(event);
                let pending = coordinator.state.take_pending_events(cancel_controller);
                for ev in pending {
                    if event_tx.send(ev).await.is_err() {
                        return;
                    }
                }
                let ui_state_events = coordinator.state.take_pending_ui_state_events();
                for ev in ui_state_events {
                    let _ = bus.ui_state().send(ev).await;
                }
                if coordinator.state.quit_requested {
                    let _ = event_tx.send(OrchestratorEvent::Quit).await;
                    return;
                }
                if let Some(msg) = coordinator.fatal_error() {
                    let _ = event_tx
                        .send(OrchestratorEvent::FatalError(LabeledError::new(
                            msg.to_string(),
                        )))
                        .await;
                    return;
                }
                coordinator.mark_render_needed();
                let _ = coordinator.render_if_needed(live_terminal);
            }
            ok = tool_rx.recv() => {
                if let Ok(event) = ok {
                    coordinator.reduce_tool_event(event);
                    coordinator.mark_render_needed();
                    let _ = coordinator.render_if_needed(live_terminal);
                }
            }
            ok = llm_rx.recv() => {
                if let Ok(event) = ok {
                    coordinator.reduce_llm_event(event);
                    coordinator.mark_render_needed();
                    let _ = coordinator.render_if_needed(live_terminal);
                }
            }
            ok = warning_rx.recv() => {
                if let Ok(event) = ok
                    && coordinator.reduce_warning_event(event)
                {
                    coordinator.mark_render_needed();
                    let _ = coordinator.render_if_needed(live_terminal);
                }
            }
            ok = compaction_rx.recv() => {
                if let Ok(event) = ok
                    && coordinator.reduce_compaction_event(event)
                {
                    coordinator.mark_render_needed();
                    let _ = coordinator.render_if_needed(live_terminal);
                }
            }
            ok = turn_rx.recv() => {
                if let Ok(event) = ok
                    && coordinator.reduce_turn_event(event)
                {
                    // A turn completion (or failure) clears the active prompt. Drain
                    // any prompts stacked during the turn into PromptSubmitted events
                    // now, without waiting for the next terminal input.
                    let pending = coordinator.state.take_pending_events(cancel_controller);
                    for ev in pending {
                        if event_tx.send(ev).await.is_err() {
                            return;
                        }
                    }
                    let ui_state_events = coordinator.state.take_pending_ui_state_events();
                    for ev in ui_state_events {
                        let _ = bus.ui_state().send(ev).await;
                    }
                    coordinator.mark_render_needed();
                    let _ = coordinator.render_if_needed(live_terminal);
                }
            }
            ok = session_rx.recv() => {
                // Session lifecycle events are not rendered in the TUI; drain only.
                let _ = ok;
            }
            ok = ui_state_rx.recv() => {
                if let Ok(event) = ok {
                    coordinator.reduce_ui_state_event(event);
                    coordinator.mark_render_needed();
                    let _ = coordinator.render_if_needed(live_terminal);
                }
            }
            ok = permission_rx.recv() => {
                if let Ok(event) = ok {
                    if let nu_agent_core::bus::PermissionEvent::Requested { context, .. } = &event {
                        crate::interaction::reducer::apply_permission_request_display(
                            &mut coordinator.state,
                            context,
                        );
                    }
                    if coordinator.state.permission.reduce_permission_event(event) {
                        coordinator.state.scroll.scroll_transcript_to_bottom();
                        coordinator.state.ensure_invariants();
                        coordinator.mark_render_needed();
                        let _ = coordinator.render_if_needed(live_terminal);
                    }
                }
            }
            _ = branch_rx.recv() => {
                coordinator.refresh_repo_branch();
                let _ = coordinator.render_if_needed(live_terminal);
            }
            _ = render_timer.tick() => {
                if coordinator.has_pending_status_message()
                    && coordinator.expire_status_message_if_due(Instant::now())
                {
                    coordinator.mark_render_needed();
                }
                if coordinator.has_active_animation() {
                    coordinator.mark_render_needed();
                }
                let _ = coordinator.render_if_needed(live_terminal);
            }
        }
    }
}
