use super::*;

pub struct TuiRuntimeRenderer<R, E>
where
    R: UiRenderer,
    E: TerminalEventSource,
{
    inner: R,
    coordinator: RuntimeCoordinator,
    event_source: E,
    live_terminal: Option<LiveTerminalUi>,
    tui_active: bool,
}

impl<R, E> TuiRuntimeRenderer<R, E>
where
    R: UiRenderer,
    E: TerminalEventSource,
{
    fn with_terminal_mode(
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
        self.coordinator.state.status_line = error.clone();
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

    #[cfg(test)]
    pub fn new_tui_active_for_test(inner: R, event_source: E, columns: u16, rows: u16) -> Self {
        Self::with_terminal_mode(inner, event_source, columns, rows, None, true)
    }

    #[cfg(test)]
    pub fn coordinator(&self) -> &RuntimeCoordinator {
        &self.coordinator
    }

    pub fn take_cancel_requested(&self) -> bool {
        self.coordinator.take_cancel_requested()
    }

    pub(crate) fn set_active_model_identity(&mut self, active_model_identity: String) {
        self.coordinator
            .set_active_model_identity(active_model_identity);
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

    pub(crate) fn take_next_mcp_toggle_request(&mut self) -> Option<McpToggleRequest> {
        self.coordinator.take_next_mcp_toggle_request()
    }

    pub(crate) fn take_next_model_switch_request(&mut self) -> Option<String> {
        self.coordinator.take_next_model_switch_request()
    }

    pub(crate) fn take_next_permission_decision_submission(
        &mut self,
    ) -> Option<PermissionDecisionSubmission> {
        self.coordinator.take_next_permission_decision_submission()
    }

    pub(crate) fn set_mcp_server_state(
        &mut self,
        server_name: &str,
        state: McpServerUsabilityState,
    ) -> bool {
        self.coordinator.set_mcp_server_state(server_name, state)
    }

    pub(crate) fn set_mcp_server_state_with_details(
        &mut self,
        server_name: &str,
        state: McpServerUsabilityState,
        reason: Option<String>,
        llm_visible_mcp_tool_count: usize,
    ) -> bool {
        self.coordinator.set_mcp_server_state_with_details(
            server_name,
            state,
            reason,
            llm_visible_mcp_tool_count,
        )
    }

    pub(crate) fn set_llm_visible_mcp_tool_count(&mut self, count: usize) {
        self.coordinator.set_llm_visible_mcp_tool_count(count);
    }

    pub(crate) fn set_mcp_visible_tool_count_by_server_name(
        &mut self,
        server_name: &str,
        count: usize,
    ) {
        self.coordinator
            .set_mcp_visible_tool_count_by_server_name(server_name, count);
    }

    pub(crate) fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        server_name: &str,
        names: Vec<String>,
    ) {
        self.coordinator
            .set_mcp_visible_tool_names_by_server_name(server_name, names);
    }

    pub(crate) fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.coordinator.set_context_window_max_tokens(max_tokens);
    }

    pub(crate) fn set_model_picker_options(&mut self, options: Vec<ModelPickerOption>) {
        self.coordinator.set_model_picker_options(options);
    }

    pub(crate) fn set_agent_picker_options(
        &mut self,
        options: Vec<nu_agent_core::protocol::picker::AgentPickerOption>,
    ) {
        self.coordinator.set_agent_picker_options(options);
    }

    pub(crate) fn set_active_agent_identity(&mut self, name: &str) {
        self.coordinator.set_active_agent_identity(name);
    }

    pub(crate) fn set_agent_cycle_names(&mut self, names: Vec<String>) {
        self.coordinator.set_agent_cycle_names(names);
    }

    pub(crate) fn set_repo_branch_caller_cwd(&mut self, caller_cwd: Option<PathBuf>) {
        self.coordinator.set_repo_branch_caller_cwd(caller_cwd);
    }

    pub(crate) fn fatal_error(&self) -> Option<&str> {
        self.coordinator.fatal_error()
    }

    pub(crate) fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
        last_total_tokens: Option<u64>,
    ) {
        self.coordinator
            .hydrate_transcript_from_messages(messages, last_total_tokens);
    }

    pub(crate) fn clear_transcript(&mut self) {
        self.coordinator.clear_transcript();
    }

    pub fn pump_terminal_once(&mut self) {
        self.coordinator.poll_terminal_event(&mut self.event_source);
        self.coordinator.drain_transport();
        if let Err(error) = self.coordinator.render_if_needed(&mut self.live_terminal) {
            self.mark_render_failure(error);
        }
    }

    pub fn emit_batch(&mut self, events: &[UiEvent]) {
        // Enqueue all events first
        for event in events {
            self.coordinator.enqueue_ui_event(event.clone());
        }
        // Then do ONE poll + drain + render cycle
        self.coordinator.poll_terminal_event(&mut self.event_source);
        self.coordinator.drain_transport();
        if let Err(error) = self.coordinator.render_if_needed(&mut self.live_terminal) {
            self.mark_render_failure(error);
        }
        // If TUI is not active, forward to inner renderer
        if !self.tui_active {
            for event in events {
                self.inner.emit(event);
            }
        }
    }

    pub(crate) fn display_incoming_message(&mut self, text: &str) {
        self.coordinator.display_incoming_message(text);
    }

    pub(crate) fn take_submitted_prompt(&mut self) -> Option<String> {
        self.coordinator.take_submitted_prompt()
    }

    pub(crate) fn take_next_model_picker_launch_request(&mut self) -> bool {
        self.coordinator.take_next_model_picker_launch_request()
    }

    pub(crate) fn take_next_agent_picker_launch_request(&mut self) -> bool {
        self.coordinator.take_next_agent_picker_launch_request()
    }

    pub(crate) fn take_next_agent_switch_request(&mut self) -> Option<String> {
        self.coordinator.take_next_agent_switch_request()
    }

    pub(crate) fn execute_shared_ui_action(&mut self, action: SharedUiAction) -> bool {
        self.coordinator.execute_shared_ui_action(action)
    }

    pub fn quit_requested(&self) -> bool {
        self.coordinator.quit_requested()
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
        if let Err(error) = self.coordinator.render_if_needed(&mut self.live_terminal) {
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
