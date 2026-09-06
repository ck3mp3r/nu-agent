use super::*;

use nu_agent_core::protocol::event::UiEvent;
use nu_agent_core::protocol::skills::DiscoverableSkill as ProtocolDiscoverableSkill;
use nu_agent_core::renderer::UiRenderer;
use nu_agent_core::tools::mcp::runtime::McpServerLifecycle;

pub struct TuiRuntimeRenderer<R, E>
where
    R: UiRenderer,
    E: TerminalEventSource,
{
    inner: R,
    pub(crate) coordinator: RuntimeCoordinator,
    event_source: E,
    live_terminal: Option<LiveTerminalUi>,
    tui_active: bool,
}

impl<R, E> TuiRuntimeRenderer<R, E>
where
    R: UiRenderer,
    E: TerminalEventSource,
{
    pub(super) fn with_terminal_mode(
        inner: R,
        event_source: E,
        columns: u16,
        rows: u16,
        live_terminal: Option<LiveTerminalUi>,
        tui_active: bool,
    ) -> Self {
        Self {
            inner,
            coordinator: RuntimeCoordinator::new(columns, rows, Some(true)),
            event_source,
            live_terminal,
            tui_active,
        }
    }

    fn mark_render_failure(&mut self, error: String) {
        self.coordinator
            .state
            .status
            .message
            .set_message(error.clone());
        self.coordinator.fatal_error = Some(error);
        self.coordinator.quit_requested = true;
    }

    pub fn new(inner: R, event_source: E, columns: u16, rows: u16) -> Self {
        Self::with_terminal_mode(inner, event_source, columns, rows, None, false)
    }

    pub fn new_live(inner: R, event_source: E, columns: u16, rows: u16) -> Result<Self, String> {
        let live_terminal = LiveTerminalUi::new()?;
        Ok(Self::with_terminal_mode(
            inner,
            event_source,
            columns,
            rows,
            Some(live_terminal),
            true,
        ))
    }

    pub fn take_cancel_requested(&self) -> bool {
        self.coordinator.take_cancel_requested()
    }

    pub(crate) fn take_live_terminal(&mut self) -> Option<LiveTerminalUi> {
        self.live_terminal.take()
    }

    pub(crate) fn set_mcp_lifecycle_projection(&mut self, projection: Vec<McpServerLifecycle>) {
        self.coordinator.set_mcp_lifecycle_projection(projection);
    }

    pub(crate) fn set_skills_projection(&mut self, skills: Vec<ProtocolDiscoverableSkill>) {
        self.coordinator.set_skills_projection(skills);
    }

    pub(crate) fn mark_skills_discovery_failed(&mut self) {
        self.coordinator.mark_skills_discovery_failed();
    }

    pub(crate) fn set_repo_branch_caller_cwd(&mut self, caller_cwd: Option<std::path::PathBuf>) {
        self.coordinator.set_repo_branch_caller_cwd(caller_cwd);
    }

    pub fn emit_batch(&mut self, events: &[UiEvent]) {
        // Enqueue all events first
        for event in events {
            self.coordinator.enqueue_ui_event(event.clone());
        }
        // Then do ONE poll + drain + render cycle
        self.coordinator.poll_terminal_event(&mut self.event_source);
        self.coordinator.drain_transport();
        let mut live = self.live_terminal.as_mut().map(|l| &mut l.terminal);
        if let Err(error) = self.coordinator.render_if_needed(&mut live) {
            self.mark_render_failure(error);
        }
        // If TUI is not active, forward to inner renderer
        if !self.tui_active {
            for event in events {
                self.inner.emit(event);
            }
        }
    }

    pub fn quit_requested(&self) -> bool {
        self.coordinator.quit_requested()
    }

    pub fn fatal_error(&self) -> Option<&str> {
        self.coordinator.fatal_error()
    }

    pub fn event_sender(
        &self,
    ) -> &tokio::sync::mpsc::Sender<nu_agent_core::orchestrator::OrchestratorEvent> {
        self.coordinator.event_sender()
    }
}

impl<R, E> UiRenderer for TuiRuntimeRenderer<R, E>
where
    R: UiRenderer,
    E: TerminalEventSource,
{
    fn emit(&mut self, event: &UiEvent) {
        self.coordinator.poll_terminal_event(&mut self.event_source);
        self.coordinator.enqueue_ui_event(event.clone());
        self.coordinator.drain_transport();
        let mut live = self.live_terminal.as_mut().map(|l| &mut l.terminal);
        if let Err(error) = self.coordinator.render_if_needed(&mut live) {
            self.mark_render_failure(error);
        }
        if !self.tui_active {
            self.inner.emit(event);
        }
    }

    fn flush(&mut self) {
        self.inner.flush();
    }
}
