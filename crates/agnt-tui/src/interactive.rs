use agnt_runtime::bus::{Control, Facts, Lifecycle, Narration, Stream};
use agnt_runtime::orchestrator::events::OrchestratorEvent;
use tokio::sync::mpsc;

use crate::interaction::input::TerminalEvent;
use crate::runtime::map_crossterm_event;
use crate::runtime::{RuntimeCoordinator, TuiRuntimeRenderer};
use crate::state::{ActivePicker, PickerOption};

mod render_loop;

pub(crate) use render_loop::run_render_loop;

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

/// Most terminal events one pass of the render loop folds. Type-ahead past the
/// bound waits for the next pass, so the bus lanes always get an arm and never
/// starve behind a burst of keys.
pub(crate) const INPUT_BURST_DRAIN_MAX: usize = 32;

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

    /// Show the instance log file this process writes in the Status popup. The
    /// path is data from the runtime; the popup row composes here.
    pub fn set_instance_log_path(&mut self, path: Option<std::path::PathBuf>) {
        self.renderer
            .coordinator
            .state
            .status
            .identity
            .set_instance_log_path(path);
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
