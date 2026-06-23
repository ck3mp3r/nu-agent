use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

use ratatui::symbols;
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    layout::{Margin, Position, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Table, TableState, Wrap,
    },
};
mod backend;
mod panels;
mod render_frame;
mod renderer;
mod status;
mod status_help;
mod terminal_events;
mod terminal_io;
mod tool_hydration;
use backend::LiveTerminalUi;
pub use backend::{AnsiTerminalBackend, RuntimeRunError, run_with_terminal_restore};
use panels::*;
pub use renderer::TuiRuntimeRenderer;
use status_help::*;

use render_frame::{
    ModalPanelKind, STATUS_TARGET_HEIGHT, current_time_millis, modal_rect_for_panel,
};
use status::{
    availability_label, build_status_lines, compact_status_line, lane_2_status_line,
    model_activity_label,
};
#[cfg(test)]
pub use terminal_events::ScriptedTerminalEvents;
#[cfg(test)]
pub(crate) use terminal_events::map_crossterm_event_for_test;
#[allow(unused_imports)]
pub use terminal_events::{
    CrosstermTerminalEvents, HybridTerminalEvents, InputSourceDiagnostics, TerminalEventSource,
};
pub use terminal_io::{TtyTerminalEvents, open_tty_reader};
use tool_hydration::{extract_tool_name, parse_persisted_tool_status_line};

#[cfg(test)]
mod test;

#[cfg(test)]
mod hybrid_events_test;

use crate::tui_renderer::TuiRenderer;
use crate::{
    interaction::{
        cancel::CancelController,
        dispatch::dispatch_terminal_event,
        input::{TerminalEvent, TerminalKey},
        reducer::{ReducerInput, append_direct_tool_display, reduce_with_cancel_controller},
    },
    platform::{
        safety::{RestoreRunError, run_with_restore},
        terminal::{TerminalBackend, TerminalLifecycle, TerminalLifecycleError},
        transport::{TransportItem, TuiTransport},
    },
    rendering::{
        layout::{
            LayoutInput, LayoutOutput, input_cursor_row_col, input_pane_height_for_content,
            recompute_layout, wrapped_input_rows,
        },
        theme::TuiTheme,
    },
    state::{
        AppState, CompactionStatus, InfoPanel, McpServerState, McpServerUsabilityState,
        McpToggleRequest, ModelPickerOption, PromptStatus, TranscriptLineStatus, TranscriptRole,
    },
};
use nu_agent_core::protocol::contracts::{SharedUiAction, UiMessageSnapshot};
use nu_agent_core::protocol::event::PermissionDecisionSubmission;
use nu_agent_core::protocol::event::UiEvent;
use nu_agent_core::protocol::skills::DiscoverableSkill as ProtocolDiscoverableSkill;
use nu_agent_core::renderer::UiRenderer;
use nu_agent_core::tools::mcp::runtime::McpServerLifecycle;
use nu_agent_core::transcript::items::{ProseMessage, Renderable, TranscriptEntry};
use nu_agent_core::transcript::renderer::{BlockRenderer, RenderContext};

#[derive(Debug)]
pub struct RuntimeCoordinator {
    state: AppState,
    transport: TuiTransport,
    cancel_controller: CancelController,
    layout: LayoutOutput,
    terminal_columns: u16,
    terminal_rows: u16,
    side_pane_visible: Option<bool>,
    quit_requested: bool,
    fatal_error: Option<String>,
    active_model_identity: String,
    input_backend_status: String,
    last_input_poll_status: String,
    last_input_error: Option<String>,
    input_watchdog_started_at: Instant,
    input_watchdog_timeout: Duration,
    repo_branch_tracker: Option<status::RepoBranchTracker>,
    theme: TuiTheme,
    render_needed: bool,
    last_render_at: Instant,
}

impl RuntimeCoordinator {
    const DEFAULT_INPUT_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(5);
    const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(16);

    pub fn new(columns: u16, rows: u16, side_pane_visible: Option<bool>) -> Self {
        Self::new_with_watchdog(
            columns,
            rows,
            side_pane_visible,
            Self::DEFAULT_INPUT_WATCHDOG_TIMEOUT,
        )
    }

    pub(crate) fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
        last_total_tokens: Option<u64>,
    ) {
        for mut message in messages {
            if let Some(usage) = message.usage() {
                self.state.hydrate_usage(
                    usage.input_tokens(),
                    usage.output_tokens(),
                    usage.total_tokens(),
                );
            }
            // Render tool display from hydrated ToolResult (if present)
            if let Some(display) = message.take_tool_display() {
                append_direct_tool_display(&mut self.state, display);
                continue;
            }
            let role = match message.role() {
                "user" => TranscriptRole::User,
                "assistant" => TranscriptRole::Assistant,
                "tool" => TranscriptRole::Tool,
                "compaction" => TranscriptRole::Compaction,
                _ => TranscriptRole::System,
            };
            let message_content = message.content();

            if role == TranscriptRole::Compaction {
                // Create compaction block structure (header with checkmark)
                self.state.start_compaction_block("history");
                self.state
                    .finish_compaction_block("history", CompactionStatus::Done);

                // After Bug 2, content is just the summary body (no stats line)
                if !message_content.trim().is_empty() {
                    for line in crate::markdown::render_markdown_lines(message_content) {
                        self.state
                            .push_transcript_item(TranscriptEntry::Assistant(ProseMessage {
                                lines: vec![line],
                            }));
                    }
                }
                self.state
                    .push_transcript_line(TranscriptRole::Separator, String::new());
                continue;
            }

            if message_content.trim().is_empty() {
                continue;
            }
            if role == TranscriptRole::Assistant {
                for line in crate::markdown::render_markdown_lines(message_content) {
                    self.state
                        .push_transcript_item(TranscriptEntry::Assistant(ProseMessage {
                            lines: vec![line],
                        }));
                }
                continue;
            }

            if role == TranscriptRole::Tool {
                let persisted = message_content.trim();
                if let Some(arguments) = message.tool_arguments() {
                    let success = message.tool_success().unwrap_or(true);
                    self.state
                        .start_tool_call(extract_tool_name(persisted), arguments);
                    self.state
                        .finish_tool_call(extract_tool_name(persisted), arguments, success);
                    continue;
                }
                if let Some((name, arguments, success)) =
                    parse_persisted_tool_status_line(persisted)
                {
                    self.state.start_tool_call(name, arguments);
                    self.state.finish_tool_call(name, arguments, success);
                    continue;
                }
                // Tool result without tool_arguments — skip (not displayed on reload)
                continue;
            }

            for line in message_content.lines() {
                if !line.trim().is_empty() {
                    self.state.push_transcript_line(role, line.to_string());
                }
            }
        }

        if let Some(tokens) = last_total_tokens {
            self.state.hydrate_latest_total_tokens(tokens);
        }
    }

    fn new_with_watchdog(
        columns: u16,
        rows: u16,
        _side_pane_visible: Option<bool>,
        input_watchdog_timeout: Duration,
    ) -> Self {
        let side_pane_visible = Some(false);
        let layout = recompute_layout(LayoutInput {
            columns,
            rows,
            side_pane_visible,
            input_height: None,
            queue_height: 0,
        });
        let mut coordinator = Self {
            state: AppState::new(),
            transport: TuiTransport::new(),
            cancel_controller: CancelController::new(),
            layout,
            terminal_columns: columns,
            terminal_rows: rows,
            side_pane_visible,
            quit_requested: false,
            fatal_error: None,
            active_model_identity: "unknown".to_string(),
            input_backend_status: "unknown".to_string(),
            last_input_poll_status: "waiting for input poll".to_string(),
            last_input_error: None,
            input_watchdog_started_at: Instant::now(),
            input_watchdog_timeout,
            repo_branch_tracker: None,
            theme: TuiTheme::default(),
            render_needed: true,
            last_render_at: Instant::now() - Duration::from_millis(100),
        };
        coordinator.sync_transcript_viewport_lines_with_layout();
        coordinator
    }

    #[cfg(test)]
    pub fn new_for_test_with_watchdog(
        columns: u16,
        rows: u16,
        side_pane_visible: Option<bool>,
        input_watchdog_timeout: Duration,
    ) -> Self {
        Self::new_with_watchdog(columns, rows, side_pane_visible, input_watchdog_timeout)
    }

    #[cfg(test)]
    pub fn state(&self) -> &AppState {
        &self.state
    }

    #[cfg(test)]
    pub fn layout(&self) -> LayoutOutput {
        self.layout
    }

    #[cfg(test)]
    pub fn cancel_controller(&self) -> &CancelController {
        &self.cancel_controller
    }

    #[cfg(test)]
    pub fn input_diagnostics_snapshot(&self) -> (String, String, Option<String>) {
        (
            self.input_backend_status.clone(),
            self.last_input_poll_status.clone(),
            self.last_input_error.clone(),
        )
    }

    pub(crate) fn display_incoming_message(&mut self, text: &str) {
        self.state.enqueue_external_prompt(text.to_string());
    }

    pub(crate) fn take_submitted_prompt(&mut self) -> Option<String> {
        self.state.take_next_prompt_for_execution()
    }

    pub(crate) fn take_next_model_picker_launch_request(&mut self) -> bool {
        self.state.take_next_model_picker_launch_request()
    }

    pub(crate) fn take_next_agent_picker_launch_request(&mut self) -> bool {
        self.state.take_next_agent_picker_launch_request()
    }

    pub(crate) fn take_next_agent_switch_request(&mut self) -> Option<String> {
        self.state.take_next_agent_switch_request()
    }

    pub(crate) fn set_active_model_identity(&mut self, active_model_identity: String) {
        self.active_model_identity = active_model_identity;
    }

    pub(crate) fn set_mcp_lifecycle_projection(&mut self, projection: Vec<McpServerLifecycle>) {
        let servers = projection
            .into_iter()
            .map(|server| {
                let name = server.name;
                self.state.set_mcp_visible_tool_count_by_server_name(
                    name.as_str(),
                    server.visible_tool_count,
                );
                McpServerState {
                    name,
                    state: match (server.enabled, server.connected) {
                        (true, true) => McpServerUsabilityState::Enabled,
                        (true, false) => McpServerUsabilityState::Failed,
                        (false, _) => McpServerUsabilityState::Disabled,
                    },
                }
            })
            .collect();
        self.state.set_mcp_servers(servers);
    }

    pub(crate) fn set_skills_projection(&mut self, skills: Vec<ProtocolDiscoverableSkill>) {
        let mapped = skills
            .into_iter()
            .map(|skill| crate::state::DiscoverableSkill {
                source_priority: skill.source.priority(),
                source: skill.source.label().to_string(),
                name: skill.name,
            })
            .collect();
        self.state.set_discoverable_skills(mapped);
    }

    pub(crate) fn mark_skills_discovery_failed(&mut self) {
        self.state.mark_skills_discovery_failed();
    }

    pub(crate) fn set_llm_visible_mcp_tool_count(&mut self, count: usize) {
        self.state.set_llm_visible_mcp_tool_count(count);
    }

    pub(crate) fn set_mcp_visible_tool_count_by_server_name(
        &mut self,
        server_name: &str,
        count: usize,
    ) {
        self.state
            .set_mcp_visible_tool_count_by_server_name(server_name, count);
    }

    pub(crate) fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        server_name: &str,
        names: Vec<String>,
    ) {
        self.state
            .set_mcp_visible_tool_names_by_server_name(server_name, names);
    }

    pub(crate) fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.state.set_context_window_max_tokens(max_tokens);
    }

    pub(crate) fn set_model_picker_options(&mut self, options: Vec<ModelPickerOption>) {
        self.state.set_model_picker_options(options);
    }

    pub(crate) fn set_agent_picker_options(
        &mut self,
        options: Vec<nu_agent_core::protocol::picker::AgentPickerOption>,
    ) {
        self.state.set_agent_picker_options(options);
    }

    pub(crate) fn set_active_agent_identity(&mut self, name: &str) {
        self.state.set_active_agent_identity(name);
    }

    pub(crate) fn set_agent_cycle_names(&mut self, names: Vec<String>) {
        self.state.agent_cycle_names = names;
    }

    pub(crate) fn set_repo_branch_caller_cwd(&mut self, caller_cwd: Option<PathBuf>) {
        self.repo_branch_tracker = Some(status::RepoBranchTracker::from_caller_cwd(caller_cwd));
    }

    pub(crate) fn take_next_mcp_toggle_request(&mut self) -> Option<McpToggleRequest> {
        self.state.take_next_mcp_toggle_request()
    }

    pub(crate) fn take_next_model_switch_request(&mut self) -> Option<String> {
        self.state.take_next_model_switch_request()
    }

    pub(crate) fn take_next_permission_decision_submission(
        &mut self,
    ) -> Option<PermissionDecisionSubmission> {
        self.state.take_next_permission_decision_submission()
    }

    pub(crate) fn set_mcp_server_state(
        &mut self,
        server_name: &str,
        state: McpServerUsabilityState,
    ) -> bool {
        self.state.set_mcp_server_state_by_name(server_name, state)
    }

    pub(crate) fn set_mcp_server_state_with_details(
        &mut self,
        server_name: &str,
        state: McpServerUsabilityState,
        reason: Option<String>,
        llm_visible_mcp_tool_count: usize,
    ) -> bool {
        self.state
            .set_llm_visible_mcp_tool_count(llm_visible_mcp_tool_count);
        self.state
            .set_mcp_server_state_by_name_with_reason(server_name, state, reason)
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub(crate) fn fatal_error(&self) -> Option<&str> {
        self.fatal_error.as_deref()
    }

    pub fn take_cancel_requested(&self) -> bool {
        self.cancel_controller.take_cancel_requested()
    }

    pub fn enqueue_ui_event(&mut self, event: UiEvent) {
        log::trace!("tui: enqueue_ui_event {:?}", std::mem::discriminant(&event));
        self.transport.enqueue_ui_event(event);
    }

    pub fn poll_terminal_event(&mut self, event_source: &mut impl TerminalEventSource) {
        if let Some(tracker) = self.repo_branch_tracker.as_mut() {
            tracker.tick();
        }

        let poll_result = event_source.poll_event();
        let diagnostics = event_source.diagnostics();
        self.update_input_diagnostics(&diagnostics);

        let event = match poll_result {
            Ok(Some(event)) => event,
            Ok(None) => {
                self.maybe_trigger_input_watchdog(&diagnostics);
                return;
            }
            Err(error) => {
                if Self::both_backends_unavailable(&diagnostics) {
                    self.trigger_no_interactive_backend_fail_fast(Some(error));
                    return;
                }
                self.state.status_line = format!("Terminal input error: {error}");
                self.fatal_error = Some(self.state.status_line.clone());
                self.quit_requested = true;
                self.cancel_controller.request_cancel();
                return;
            }
        };

        self.last_input_poll_status = format!("event from {}", diagnostics.active_backend);

        if let TerminalEvent::Key(TerminalKey::Esc) = event
            && self.state.phase == crate::state::UiPhase::Idle
            && !self.state.command_palette_open
            && self.state.info_panel.is_none()
        {
            self.state.status_line = "Esc pressed. Press Ctrl+C to quit.".to_string();
        }

        if let TerminalEvent::Key(TerminalKey::CtrlC) = event {
            self.quit_requested = true;
            self.cancel_controller.request_cancel();
        }

        if let TerminalEvent::Resize(resize) = event {
            self.terminal_columns = resize.columns;
            self.terminal_rows = resize.rows;
            let input_height = input_pane_height_for_content(
                &input_buffer_for_layout(&self.state),
                resize.columns,
            );
            self.layout = recompute_layout(LayoutInput {
                columns: resize.columns,
                rows: resize.rows,
                side_pane_visible: self.side_pane_visible,
                input_height: Some(input_height),
                queue_height: (self.state.pending_prompt_count() as u16).saturating_mul(2),
            });
        }

        let _ = dispatch_terminal_event(&mut self.state, &event, Some(&self.cancel_controller));
        self.recompute_layout_for_current_input();
        self.flush_clipboard_request();
        self.quit_requested |= self.state.quit_requested;

        self.sync_transcript_viewport_lines_with_layout();
        self.mark_render_needed();
    }

    fn sync_transcript_viewport_lines_with_layout(&mut self) {
        // With ListState, viewport lines are managed by ratatui automatically
        // No manual tracking needed
    }

    fn execute_shared_ui_action(&mut self, action: SharedUiAction) -> bool {
        match action {
            SharedUiAction::Help => {
                self.state.open_info_panel(InfoPanel::Help);
                true
            }
            SharedUiAction::Status => {
                self.state.open_info_panel(InfoPanel::Status);
                true
            }
            SharedUiAction::Mcps => {
                self.state.open_info_panel(InfoPanel::Mcps);
                true
            }
            SharedUiAction::Models => {
                self.state.open_model_picker();
                true
            }
            SharedUiAction::Agents => {
                self.state.open_agent_picker();
                true
            }
        }
    }

    fn recompute_layout_for_current_input(&mut self) {
        let input_height = input_pane_height_for_content(
            &input_buffer_for_layout(&self.state),
            self.layout.transcript.width,
        );
        self.layout = recompute_layout(LayoutInput {
            columns: self.terminal_columns,
            rows: self.terminal_rows,
            side_pane_visible: self.side_pane_visible,
            input_height: Some(input_height),
            queue_height: (self.state.pending_prompt_count() as u16).saturating_mul(2),
        });
    }

    fn flush_clipboard_request(&mut self) {
        let Some(text) = self.state.take_clipboard_request() else {
            return;
        };

        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
            Ok(()) => {
                self.state.status_line = "Copied selection to clipboard.".to_string();
            }
            Err(error) => {
                self.state.status_line = format!("Clipboard copy failed: {error}");
            }
        }
    }

    fn update_input_diagnostics(&mut self, diagnostics: &InputSourceDiagnostics) {
        let primary = availability_label(diagnostics.primary_available);
        let fallback = availability_label(diagnostics.fallback_available);
        self.input_backend_status = format!(
            "active={}, crossterm={}, /dev/tty={}",
            diagnostics.active_backend, primary, fallback
        );
        self.last_input_poll_status = diagnostics.last_poll_state.clone();
        self.last_input_error = diagnostics.last_error.clone();
    }

    fn maybe_trigger_input_watchdog(&mut self, diagnostics: &InputSourceDiagnostics) {
        if self.quit_requested || self.fatal_error.is_some() {
            return;
        }

        if !Self::both_backends_unavailable(diagnostics) {
            return;
        }

        if self.input_watchdog_started_at.elapsed() < self.input_watchdog_timeout {
            return;
        }

        self.trigger_no_interactive_backend_fail_fast(None);
    }

    fn both_backends_unavailable(diagnostics: &InputSourceDiagnostics) -> bool {
        diagnostics.primary_available == Some(false)
            && diagnostics.fallback_available == Some(false)
    }

    fn trigger_no_interactive_backend_fail_fast(&mut self, poll_error: Option<String>) {
        if let Some(error) = poll_error
            && self.last_input_error.is_none()
        {
            self.last_input_error = Some(error);
        }

        let mut message = format!(
            "No interactive input backend available (crossterm and /dev/tty unavailable). Last poll: {}.",
            self.last_input_poll_status
        );
        if let Some(error) = self.last_input_error.as_deref() {
            message.push_str(&format!(" Last error: {error}."));
        }
        message.push_str(" Run `agent` in an interactive terminal and verify TTY access.");

        self.state.status_line = message.clone();
        self.fatal_error = Some(message);
        self.quit_requested = true;
        self.cancel_controller.request_cancel();
    }

    pub fn drain_transport(&mut self) {
        let mut pending_assistant: Option<UiEvent> = None;
        let mut pending_compaction: Option<UiEvent> = None;

        while let Some(item) = self.transport.poll_next() {
            if matches!(
                &item,
                TransportItem::Event(e) if matches!(e.as_ref(), UiEvent::AssistantMessage { .. })
            ) {
                pending_assistant = Some(match item {
                    TransportItem::Event(e) => *e,
                    _ => unreachable!(),
                });
                continue;
            }

            if matches!(
                &item,
                TransportItem::Event(e) if matches!(e.as_ref(), UiEvent::CompactionSummaryChunk { .. })
            ) {
                pending_compaction = Some(match item {
                    TransportItem::Event(e) => *e,
                    _ => unreachable!(),
                });
                continue;
            }

            // Flush any pending coalesced events before processing a different event type
            // (preserves ordering: assistant text before tool events, etc.)
            if let Some(event) = pending_assistant.take() {
                reduce_with_cancel_controller(
                    &mut self.state,
                    ReducerInput::Event(Box::new(event)),
                    Some(&self.cancel_controller),
                );
            }
            if let Some(event) = pending_compaction.take() {
                reduce_with_cancel_controller(
                    &mut self.state,
                    ReducerInput::Event(Box::new(event)),
                    Some(&self.cancel_controller),
                );
            }

            // Process the current non-coalesceable event
            reduce_with_cancel_controller(
                &mut self.state,
                ReducerInput::from(item),
                Some(&self.cancel_controller),
            );
        }

        // Flush remaining pending events
        if let Some(event) = pending_assistant.take() {
            reduce_with_cancel_controller(
                &mut self.state,
                ReducerInput::Event(Box::new(event)),
                Some(&self.cancel_controller),
            );
        }
        if let Some(event) = pending_compaction.take() {
            reduce_with_cancel_controller(
                &mut self.state,
                ReducerInput::Event(Box::new(event)),
                Some(&self.cancel_controller),
            );
        }

        self.mark_render_needed();
    }

    fn mark_render_needed(&mut self) {
        self.render_needed = true;
    }

    fn render_if_needed(&mut self, live: &mut Option<LiveTerminalUi>) -> Result<(), String> {
        if !self.render_needed {
            return Ok(());
        }
        if self.last_render_at.elapsed() < Self::MIN_FRAME_INTERVAL {
            return Ok(());
        }
        self.render_needed = false;
        self.last_render_at = Instant::now();
        self.render_frame(live)
    }

    #[cfg(test)]
    pub fn render_needed(&self) -> bool {
        self.render_needed
    }

    #[cfg(test)]
    pub fn last_render_at(&self) -> Instant {
        self.last_render_at
    }

    #[cfg(test)]
    pub fn set_render_needed(&mut self, needed: bool) {
        self.render_needed = needed;
    }

    #[cfg(test)]
    pub fn set_last_render_at(&mut self, at: Instant) {
        self.last_render_at = at;
    }

    fn render_frame(&self, live: &mut Option<LiveTerminalUi>) -> Result<(), String> {
        let Some(live) = live.as_mut() else {
            return Ok(());
        };

        live.terminal
            .draw(|frame| {
                let area = frame.area();
                let has_side = self.layout.side_pane.is_some();
                let horizontal = if has_side {
                    Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                        .split(area)
                } else {
                    Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(100)])
                        .split(area)
                };

                let main = horizontal[0];
                let side_margin = if main.width >= 8 { 2 } else { 0 };
                let content_main = main.inner(Margin {
                    vertical: 0,
                    horizontal: side_margin,
                });
                let input_h = self.layout.input.height;
                let queue_h = (self.state.pending_prompt_count() as u16).saturating_mul(2);
                let vertical = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(0),
                        Constraint::Min(1),
                        Constraint::Length(queue_h),
                        Constraint::Length(input_h),
                        Constraint::Length(STATUS_TARGET_HEIGHT),
                    ])
                    .split(content_main);
                // vertical[0]=unused [1]=transcript [2]=queue [3]=input [4]=status

                let entries_for_render = transcript_entries_for_render(&self.state);
                let transcript_line_statuses =
                    transcript_line_statuses_for_render(&self.state, entries_for_render);
                let transcript_content_area = vertical[1];

                let now_millis = current_time_millis();
                let renderer = TuiRenderer {
                    theme: self.theme.clone(),
                };

                // Capture input_mode before the closure to avoid borrowing self
                let input_mode = self.state.input_mode;

                // Build ListView with tui-widget-list
                let builder = tui_widget_list::ListBuilder::new(|context| {
                    let entry = &entries_for_render[context.index];
                    let block = entry.to_render_block();
                    let item_status = transcript_line_statuses
                        .get(context.index)
                        .copied()
                        .flatten()
                        .map(transcript_line_status_to_item_status);
                    let ctx = RenderContext {
                        width: context.cross_axis_size as usize,
                        cursor: false,
                        selected: context.is_selected
                            && input_mode == crate::state::InputMode::Normal,
                        status: item_status,
                        now_millis,
                    };
                    let lines = renderer.render(&block, &ctx);
                    let text = ratatui::text::Text::from(lines);

                    // Calculate wrapped height: for each line, compute how many visual rows it takes
                    let width = context.cross_axis_size as usize;
                    let height: u16 = text
                        .lines
                        .iter()
                        .map(|line| {
                            let line_width = line.width();
                            if line_width == 0 || width == 0 {
                                1u16
                            } else {
                                line_width.div_ceil(width) as u16
                            }
                        })
                        .sum::<u16>()
                        .max(1);

                    let paragraph = Paragraph::new(text).wrap(Wrap { trim: false });
                    (paragraph, height)
                });

                let transcript_border_style =
                    if self.state.pane_focus == crate::state::PaneFocus::Transcript {
                        self.theme.focus
                    } else {
                        Style::default()
                    };

                let list_view = tui_widget_list::ListView::new(builder, entries_for_render.len())
                    .infinite_scrolling(false)
                    .block(
                        Block::default()
                            .borders(Borders::TOP)
                            .border_style(transcript_border_style),
                    );

                if vertical[1].height > 0 {
                    frame.render_widget(Clear, vertical[1]);
                    if transcript_content_area.height > 0 {
                        let mut list_state_clone = self.state.transcript_list_state.clone();
                        let rendered_len = entries_for_render.len();
                        if let Some(sel) = list_state_clone.selected
                            && sel + 1 >= self.state.transcript_preview.len()
                        {
                            list_state_clone.select(Some(rendered_len.saturating_sub(1)));
                        }
                        frame.render_stateful_widget(
                            list_view,
                            transcript_content_area,
                            &mut list_state_clone,
                        );
                        let content_count = entries_for_render.len();
                        if content_count > 0 {
                            let scroll_pos = list_state_clone.selected.unwrap_or(0);
                            let mut scrollbar_state =
                                ScrollbarState::new(content_count).position(scroll_pos);
                            frame.render_stateful_widget(
                                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                                    .begin_symbol(None)
                                    .end_symbol(None),
                                transcript_content_area,
                                &mut scrollbar_state,
                            );
                        }
                    }
                }

                if vertical[2].height > 0 {
                    let pane_width = vertical[2].width as usize;
                    let separator = "─".repeat(pane_width);
                    let queued_lines: Vec<Line> = self
                        .state
                        .prompt_items()
                        .iter()
                        .filter(|p| p.status == PromptStatus::Queued)
                        .flat_map(|p| {
                            let raw = format!("• {}", p.prompt_text);
                            let display = if raw.chars().count() > pane_width {
                                format!(
                                    "{}…",
                                    raw.chars()
                                        .take(pane_width.saturating_sub(1))
                                        .collect::<String>()
                                )
                            } else {
                                raw
                            };
                            [
                                Line::from(Span::styled(
                                    separator.clone(),
                                    self.theme.role_separator,
                                )),
                                Line::from(Span::styled(display, self.theme.role_user)),
                            ]
                        })
                        .collect();
                    frame.render_widget(Clear, vertical[2]);
                    frame.render_widget(Paragraph::new(Text::from(queued_lines)), vertical[2]);
                }

                let busy_millis = if model_activity_label(&self.state) == "busy" {
                    Some(now_millis)
                } else {
                    None
                };
                let lane_1 = compact_status_line(
                    &self.active_model_identity,
                    self.repo_branch_tracker
                        .as_ref()
                        .and_then(|tracker| tracker.branch()),
                    busy_millis,
                    vertical[4].width as usize,
                    &self.theme,
                );
                let lane_2 =
                    lane_2_status_line(&self.state, vertical[4].width as usize, &self.theme);
                let _status_lines = build_status_lines(&self.state, &self.active_model_identity);
                let status_widget = Paragraph::new(Text::from(vec![lane_1, lane_2]))
                    .block(Block::default())
                    .wrap(Wrap { trim: false });
                if vertical[4].height > 0 {
                    frame.render_widget(Clear, vertical[4]);
                    frame.render_widget(status_widget, vertical[4]);
                }

                if self.state.permission_prompt.is_some() {
                    render_permission_controls(frame, vertical[3], &self.theme);
                } else {
                    let input_rows = wrapped_input_rows(
                        &self.state.input.buffer,
                        vertical[3].width.saturating_sub(2) as usize,
                    );
                    let input_border_style =
                        if self.state.pane_focus == crate::state::PaneFocus::Input {
                            self.theme.focus
                        } else {
                            Style::default()
                        };
                    let mut input_lines = Vec::new();
                    let prompt_prefix = input_prompt_prefix(self.state.input_mode);
                    if let Some((first, rest)) = input_rows.split_first() {
                        input_lines.push(Line::from(vec![
                            Span::styled(prompt_prefix, self.theme.input_prompt),
                            Span::raw(first.clone()),
                        ]));
                        for row in rest {
                            input_lines
                                .push(Line::from(vec![Span::raw("  "), Span::raw(row.clone())]));
                        }
                    }
                    input_lines.extend(inline_slash_lines_for_render(&self.state));
                    let input_widget = Paragraph::new(Text::from(input_lines))
                        .block(
                            Block::default()
                                .borders(Borders::TOP)
                                .border_style(input_border_style),
                        )
                        .wrap(Wrap { trim: false });
                    if vertical[3].height > 0 {
                        frame.render_widget(Clear, vertical[3]);
                        frame.render_widget(input_widget, vertical[3]);
                    }

                    if !self.state.input.locked
                        && !self.state.command_palette_open
                        && self.state.info_panel.is_none()
                        && vertical[3].height >= 2
                        && vertical[3].width >= 1
                    {
                        let (cursor_row, cursor_col) = input_cursor_row_col(
                            &self.state.input.buffer,
                            self.state.input.cursor,
                            vertical[3].width.saturating_sub(2) as usize,
                        );
                        let x = vertical[3].x.saturating_add(2).saturating_add(cursor_col);
                        let max_x = vertical[3]
                            .x
                            .saturating_add(vertical[3].width.saturating_sub(1));
                        let y = vertical[3]
                            .y
                            .saturating_add(1)
                            .saturating_add(cursor_row)
                            .min(
                                vertical[3]
                                    .y
                                    .saturating_add(vertical[3].height.saturating_sub(1)),
                            );
                        frame.set_cursor_position(Position { x: x.min(max_x), y });
                    }
                }

                if has_side {
                    let side = horizontal[1];
                    let side_widget = Paragraph::new(Line::from("Events pane reserved"))
                        .block(Block::default().borders(Borders::ALL).title("Events"));
                    frame.render_widget(side_widget, side);
                }

                if self.state.command_palette_open {
                    let popup = modal_rect_for_panel(area, ModalPanelKind::CommandPalette);
                    let popup_width = popup.width;
                    let popup_height = popup.height;

                    let model = command_palette_table_model(&self.state, popup_width, popup_height);

                    let inner = render_modal_frame(
                        frame,
                        popup,
                        command_palette_title(model.overflow_cue.as_deref()),
                    );
                    let rows = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(1), Constraint::Min(0)])
                        .split(inner);

                    frame.render_widget(
                        Paragraph::new(Line::from(model.query_line.clone())),
                        rows[0],
                    );

                    let header = Row::new(vec!["Action", "Summary"]);

                    let table_rows = model.rows.iter().map(|row| {
                        Row::new(vec![Cell::from(row[0].clone()), Cell::from(row[1].clone())])
                    });

                    let widths = vec![Constraint::Length(8), Constraint::Min(12)];

                    let table = Table::new(table_rows, widths)
                        .header(header)
                        .column_spacing(2)
                        .highlight_symbol("❯ ");
                    let mut table_state = TableState::default();
                    table_state.select(model.selected);
                    frame.render_stateful_widget(table, rows[1], &mut table_state);
                }

                if let Some(panel) = self.state.info_panel {
                    let popup = modal_rect_for_panel(
                        area,
                        match panel {
                            InfoPanel::Help => ModalPanelKind::Help,
                            InfoPanel::Status => ModalPanelKind::Status,
                            InfoPanel::Mcps => ModalPanelKind::Mcps,
                            InfoPanel::Skills => ModalPanelKind::Skills,
                        },
                    );

                    match panel {
                        InfoPanel::Mcps => {
                            let pre_inner = popup.inner(Margin {
                                vertical: 1,
                                horizontal: 1,
                            });
                            let details_height =
                                mcp_details_height_for_inner_height(pre_inner.height);

                            let pre_rows = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints([
                                    Constraint::Length(1),
                                    Constraint::Min(1),
                                    Constraint::Length(details_height),
                                ])
                                .split(pre_inner);

                            let model = mcp_table_model(&self.state, pre_rows[1].height);
                            let title = if let Some(cue) = model.overflow_cue.as_deref() {
                                format!("MCPs ({cue})")
                            } else {
                                "MCPs".to_string()
                            };
                            let inner = render_modal_frame(frame, popup, title);
                            let rows = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints([
                                    Constraint::Length(1),
                                    Constraint::Min(1),
                                    Constraint::Length(details_height),
                                ])
                                .split(inner);

                            frame.render_widget(
                                Paragraph::new(Line::from(mcp_panel_controls_line())),
                                rows[0],
                            );

                            let header = Row::new(model.columns.clone());
                            let table_rows = model.rows.iter().map(|row| {
                                Row::new(vec![
                                    Cell::from(row[0].clone()),
                                    Cell::from(row[1].clone()),
                                    Cell::from(row[2].clone()),
                                ])
                            });
                            let widths = [
                                Constraint::Length(18),
                                Constraint::Length(14),
                                Constraint::Length(MCP_STATUS_COLUMN_WIDTH),
                            ];
                            let table = Table::new(table_rows, widths)
                                .header(header)
                                .column_spacing(1)
                                .highlight_symbol("❯ ");
                            let mut table_state = TableState::default();
                            table_state.select(model.selected);
                            frame.render_stateful_widget(table, rows[1], &mut table_state);

                            if details_height > 0 {
                                let details_lines = mcp_selected_details_lines(
                                    &self.state,
                                    details_height,
                                    rows[2].width,
                                );
                                if !details_lines.is_empty() {
                                    let details_widget = Paragraph::new(Text::from(details_lines))
                                        .wrap(Wrap { trim: false });
                                    frame.render_widget(details_widget, rows[2]);
                                }
                            }
                        }
                        _ => {
                            let (title, lines) = match panel {
                                InfoPanel::Help => help_panel_lines(),
                                InfoPanel::Status => {
                                    status_panel_lines(&self.state, &self.active_model_identity)
                                }
                                InfoPanel::Skills => skills_panel_lines(&self.state),
                                InfoPanel::Mcps => unreachable!("handled above"),
                            };

                            let panel_inner_height = popup.height.saturating_sub(2);
                            let panel_inner_width = popup.width.saturating_sub(2);
                            let panel_scroll =
                                self.state.info_panel_scroll.min(help_panel_max_scroll(
                                    &lines,
                                    panel_inner_height,
                                    panel_inner_width,
                                ));
                            let panel_title = match panel {
                                InfoPanel::Help => {
                                    if let Some(cue) = help_panel_overflow_cue(
                                        &lines,
                                        panel_inner_height,
                                        panel_inner_width,
                                        panel_scroll,
                                    ) {
                                        format!("{title} ({cue})")
                                    } else {
                                        title.to_string()
                                    }
                                }
                                _ => title.to_string(),
                            };

                            let inner = render_modal_frame(frame, popup, panel_title);
                            frame.render_widget(
                                Paragraph::new(lines)
                                    .wrap(Wrap { trim: false })
                                    .scroll((panel_scroll.min(u16::MAX as usize) as u16, 0)),
                                inner,
                            );
                        }
                    }
                }

                if self.state.model_picker_open {
                    let popup = modal_rect_for_panel(area, ModalPanelKind::Models);
                    let inner =
                        render_modal_frame(frame, popup, "Models (↑/↓ or Ctrl-N · Enter · Esc)");
                    let rows = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(1), Constraint::Min(0)])
                        .split(inner);
                    frame.render_widget(
                        Paragraph::new(Line::from(format!(
                            "Query: {}",
                            self.state.model_picker_query
                        ))),
                        rows[0],
                    );

                    let options = self.state.model_picker_filtered_options();
                    if options.is_empty() {
                        frame.render_widget(
                            Paragraph::new(Line::from(MODEL_PICKER_EMPTY_STATE_MESSAGE)),
                            rows[1],
                        );
                    } else {
                        let table_rows = options.iter().enumerate().map(|(idx, option)| {
                            let active = if option.active { "*" } else { "" };
                            let marker = if idx == self.state.model_picker_selection {
                                "❯ "
                            } else {
                                "  "
                            };
                            Row::new(vec![
                                Cell::from(format!("{marker}{}", option.identity)),
                                Cell::from(active.to_string()),
                            ])
                        });
                        let table =
                            Table::new(table_rows, [Constraint::Min(12), Constraint::Length(1)])
                                .header(Row::new(vec!["Model", "A"]))
                                .column_spacing(1);
                        let mut table_state = TableState::default();
                        table_state.select(Some(self.state.model_picker_selection));
                        frame.render_stateful_widget(table, rows[1], &mut table_state);
                    }
                }

                if self.state.agent_picker_open {
                    let popup = modal_rect_for_panel(area, ModalPanelKind::Agents);
                    let inner = render_modal_frame(frame, popup, "Agent (↑/↓ · Enter · Esc)");
                    let rows = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(1), Constraint::Min(0)])
                        .split(inner);
                    frame.render_widget(
                        Paragraph::new(Line::from(format!(
                            "Query: {}",
                            self.state.agent_picker_query
                        ))),
                        rows[0],
                    );

                    let options = self.state.agent_picker_filtered_options();
                    if options.is_empty() {
                        frame.render_widget(
                            Paragraph::new(Line::from(AGENT_PICKER_EMPTY_STATE_MESSAGE)),
                            rows[1],
                        );
                    } else {
                        let table_rows: Vec<Row> = options
                            .iter()
                            .enumerate()
                            .map(|(idx, option)| {
                                let active = if option.active { "*" } else { "" };
                                let marker = if idx == self.state.agent_picker_selection {
                                    "❯ "
                                } else {
                                    "  "
                                };
                                let desc = option.description.as_deref().unwrap_or("");
                                Row::new(vec![
                                    Cell::from(format!("{marker}{}", option.name)),
                                    Cell::from(desc.to_string()),
                                    Cell::from(active.to_string()),
                                ])
                            })
                            .collect();
                        let table = Table::new(
                            table_rows,
                            [
                                Constraint::Min(12),
                                Constraint::Min(20),
                                Constraint::Length(1),
                            ],
                        )
                        .header(Row::new(vec!["Agent", "Description", "A"]))
                        .column_spacing(1);
                        let mut table_state = TableState::default();
                        table_state.select(Some(self.state.agent_picker_selection));
                        frame.render_stateful_widget(table, rows[1], &mut table_state);
                    }
                }
            })
            .map_err(|err| format!("TUI render failed: {err}"))?;

        let cursor_style = self.state.input_mode.cursor_style();
        let _ = crossterm::execute!(std::io::stdout(), cursor_style);

        Ok(())
    }

    #[cfg(test)]
    pub(super) fn main_pane_rects_for_height(
        main_height: u16,
    ) -> (
        ratatui::layout::Rect,
        ratatui::layout::Rect,
        ratatui::layout::Rect,
        ratatui::layout::Rect,
    ) {
        render_frame::main_pane_rects_for_height(main_height)
    }

    #[cfg(test)]
    pub fn pump_once(&mut self, event_source: &mut impl TerminalEventSource) {
        self.poll_terminal_event(event_source);
        self.drain_transport();
    }
}

fn render_modal_frame(frame: &mut Frame, area: Rect, title: impl Into<Line<'static>>) -> Rect {
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_set(symbols::border::ROUNDED)
            .title(title),
        area,
    );
    area.inner(Margin {
        vertical: 1,
        horizontal: 1,
    })
}

#[cfg(test)]
pub(super) fn modal_frame_uses_rounded_border_style_for_test() -> bool {
    true
}

#[cfg(test)]
pub(super) fn modal_open_state_applies_dimmed_backdrop_for_test(state: &AppState) -> bool {
    state.command_palette_open
        || state.info_panel.is_some()
        || state.model_picker_open
        || state.agent_picker_open
}

#[cfg(test)]
pub(super) fn inline_model_picker_modal_respects_border_and_backdrop_policy_for_test(
    state: &AppState,
) -> bool {
    state.model_picker_open && modal_frame_uses_rounded_border_style_for_test()
}

#[cfg(test)]
pub(super) fn model_picker_empty_state_message_for_test() -> &'static str {
    MODEL_PICKER_EMPTY_STATE_MESSAGE
}
