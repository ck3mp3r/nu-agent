use std::{
    io::Write,
    time::{Duration, Instant},
};

use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    layout::{Margin, Rect},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
};
mod backend;
mod panels;
pub(crate) mod render;
mod renderer;
mod session_picker;
mod status;
mod terminal;
pub(crate) use backend::LiveTerminalUi;
pub use backend::{
    AnsiTerminalBackend, RuntimeRunError, run_with_terminal_restore, run_with_terminal_restore_sync,
};
use panels::*;
use render::frame::current_time_millis;
pub use renderer::TuiRuntimeRenderer;
use status::help::*;
use status::{availability_label, status_left_content, status_right_content};
#[cfg(test)]
pub use terminal::CrosstermTerminalEvents;
#[cfg(test)]
pub use terminal::events_test::ScriptedTerminalEvents;
#[cfg(test)]
pub(crate) use terminal::events_test::map_crossterm_event_for_test;
pub(crate) use terminal::map_crossterm_event;
pub use terminal::{HybridTerminalEvents, InputSourceDiagnostics, TerminalEventSource};
pub use terminal::{TtyTerminalEvents, open_tty_reader};

#[cfg(test)]
pub(super) mod test;

#[cfg(test)]
mod panels_test;

#[cfg(test)]
mod renderer_test;

#[cfg(test)]
mod session_picker_test;

#[cfg(test)]
pub(crate) use status::help_test::*;

use crate::{
    interaction::{
        cancel::CancelController,
        dispatch::dispatch_terminal_event,
        dispatch::rewrite_action,
        input::{TerminalEvent, TerminalKey},
        reducer::{
            ReducerInput, UserAction, append_direct_tool_display, reduce_with_cancel_controller,
        },
    },
    platform::{
        safety::{RestoreRunError, run_with_restore},
        terminal::{TerminalBackend, TerminalLifecycle, TerminalLifecycleError},
        transport::{TransportItem, TuiTransport},
    },
    rendering::{
        layout::{INPUT_MAX_HEIGHT, INPUT_MIN_HEIGHT, MAIN_SIDE_MARGIN},
        theme::{ThemeName, TuiTheme},
    },
    state::{
        AppState, CompactionStatus, InputMode, McpServerState, McpServerUsabilityState,
        ModelPickerOption, TranscriptRole,
    },
};
use nu_agent_core::orchestrator::UiStateEvent;
use nu_agent_core::protocol::contracts::UiMessageSnapshot;
use nu_agent_core::protocol::event::UiEvent;
use nu_agent_core::protocol::skills::DiscoverableSkill as ProtocolDiscoverableSkill;
use nu_agent_core::renderer::UiRenderer;
use nu_agent_core::tools::mcp::runtime::McpServerLifecycle;
use nu_agent_core::transcript::ir::Role;
use nu_agent_core::transcript::items::{ProseMessage, TranscriptEntry, TranscriptEntryKind};

#[derive(Debug)]
pub struct RuntimeCoordinator {
    pub(crate) state: AppState,
    transport: TuiTransport,
    pub(crate) cancel_controller: CancelController,
    input_height: u16,
    side_pane_visible: Option<bool>,
    quit_requested: bool,
    fatal_error: Option<String>,
    pub(crate) active_model_identity: String,
    input_backend_status: String,
    last_input_poll_status: String,
    last_input_error: Option<String>,
    input_watchdog_started_at: Instant,
    input_watchdog_timeout: Duration,
    repo_branch_tracker: Option<status::RepoBranchTracker>,
    theme: TuiTheme,
    theme_name: ThemeName,
    render_needed: bool,
    last_render_at: Instant,
    textarea: ratatui_textarea::TextArea<'static>,
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

    pub fn hydrate_transcript_from_messages(
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
                    self.state.push_transcript_item(TranscriptEntry {
                        id: 0,
                        kind: TranscriptEntryKind::Assistant(ProseMessage {
                            markdown: message_content.to_string(),
                        }),
                        status: None,
                    });
                }
                self.state.push_spacer();
                continue;
            }

            if message_content.trim().is_empty() {
                continue;
            }
            if role == TranscriptRole::Assistant {
                self.push_block_start_spacers(role);
                self.state.push_transcript_item(TranscriptEntry {
                    id: 0,
                    kind: TranscriptEntryKind::Assistant(ProseMessage {
                        markdown: message_content.trim().to_string(),
                    }),
                    status: None,
                });
                self.state.push_spacer(); // closing spacer for assistant block
                continue;
            }

            if role == TranscriptRole::Tool {
                let persisted = message_content.trim();
                if let Some(arguments) = message.tool_arguments() {
                    let success = message.tool_success().unwrap_or(true);
                    let name = message
                        .tool_name()
                        .unwrap_or_else(|| crate::state::extract_tool_name(persisted));
                    self.push_tool_block_start_spacers();
                    self.state.start_tool_call(name, arguments);
                    self.state.finish_tool_call(name, arguments, success);
                    continue;
                }
                if let Some((name, arguments, success)) =
                    crate::state::parse_persisted_tool_status_line(persisted)
                {
                    self.push_tool_block_start_spacers();
                    self.state.start_tool_call(name, arguments);
                    self.state.finish_tool_call(name, arguments, success);
                    continue;
                }
                // Tool result without tool_arguments — skip (not displayed on reload)
                continue;
            }

            // User and System messages
            self.push_block_start_spacers(role);
            for line in message_content.lines() {
                if !line.trim().is_empty() {
                    self.state.push_transcript_line(role, line.to_string());
                }
            }
            self.state.push_spacer(); // closing spacer for user/system block
        }

        // If the transcript ends with an open tool block, close it with a
        // trailing spacer (mirrors the live `finalize` behaviour).
        if self.tool_block_is_open() {
            self.state.push_spacer();
        }

        if let Some(tokens) = last_total_tokens {
            self.state.hydrate_latest_total_tokens(tokens);
        }
    }

    /// Push the closing spacer for the previous block (if not already a Spacer)
    /// followed by the starting spacer for a new block. Tool blocks are treated
    /// as closed when this runs — tool calls within a block never get spacers
    /// between them.
    fn push_block_start_spacers(&mut self, role: TranscriptRole) {
        // Check if previous block was a tool block (skip spacers to find last content)
        let last_content = self
            .state
            .transcript_preview
            .iter()
            .rev()
            .find(|e| !matches!(e.kind, TranscriptEntryKind::Spacer(_)))
            .map(|e| e.role());
        let prev_is_tool_block = matches!(last_content, Some(Role::Tool) | Some(Role::ToolDisplay));

        if role == TranscriptRole::Assistant && prev_is_tool_block {
            // Only ONE spacer between tool block and assistant
            self.state.push_spacer();
            return;
        }

        let prev_is_spacer = self
            .state
            .transcript_preview
            .last()
            .is_some_and(|last| matches!(last.kind, TranscriptEntryKind::Spacer(_)));
        // Only push a closing spacer if there is a previous block to close.
        if !self.state.transcript_preview.is_empty() && !prev_is_spacer {
            self.state.push_spacer(); // closing spacer for previous block
        }
        self.state.push_spacer(); // starting spacer for new block
    }

    /// Push the closing spacer (if not already a Spacer) + starting spacer when
    /// starting a new tool block, and nothing when continuing an open tool block.
    fn push_tool_block_start_spacers(&mut self) {
        if self.tool_block_is_open() {
            return;
        }
        // Check if previous block was an assistant block (skip spacers to find last content)
        let last_content = self
            .state
            .transcript_preview
            .iter()
            .rev()
            .find(|e| !matches!(e.kind, TranscriptEntryKind::Spacer(_)))
            .map(|e| e.role());
        let prev_is_assistant = matches!(last_content, Some(Role::Assistant));

        if prev_is_assistant {
            // Only ONE spacer between assistant and tool block
            // If the assistant block's closing spacer is already there, don't add another
            let prev_is_spacer = self
                .state
                .transcript_preview
                .last()
                .is_some_and(|last| matches!(last.kind, TranscriptEntryKind::Spacer(_)));
            if !prev_is_spacer {
                self.state.push_spacer();
            }
            return;
        }

        self.push_block_start_spacers(TranscriptRole::Tool);
    }

    /// Whether the last hydrated entry is a tool call, meaning a tool block is
    /// currently open and awaiting its closing spacer.
    fn tool_block_is_open(&self) -> bool {
        self.state.transcript_preview.last().is_some_and(|last| {
            matches!(
                last.kind,
                TranscriptEntryKind::Tool(_) | TranscriptEntryKind::ToolResult(_)
            )
        })
    }
    fn new_with_watchdog(
        _columns: u16,
        _rows: u16,
        _side_pane_visible: Option<bool>,
        input_watchdog_timeout: Duration,
    ) -> Self {
        let side_pane_visible = Some(false);
        let theme = TuiTheme::default();
        let (event_tx, _event_rx) =
            tokio::sync::mpsc::channel::<nu_agent_core::orchestrator::OrchestratorEvent>(256);
        let mut coordinator = Self {
            state: AppState::new_with_sender(event_tx),
            transport: TuiTransport::new(),
            cancel_controller: CancelController::new(),
            input_height: INPUT_MIN_HEIGHT,
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
            theme: theme.clone(),
            theme_name: ThemeName::default(),
            render_needed: true,
            last_render_at: Instant::now() - Duration::from_millis(100),
            textarea: ratatui_textarea::TextArea::default(),
        };
        coordinator.state.theme = theme;
        coordinator.sync_transcript_viewport_lines_with_layout();
        coordinator
    }
    pub(crate) fn take_next_theme_switch_request(&mut self) -> Option<String> {
        self.state.take_next_theme_switch_request()
    }

    pub fn set_active_model_identity(&mut self, active_model_identity: String) {
        self.active_model_identity = active_model_identity;
    }

    /// Consume a UI-state event from the bus. Keeps the coordinator's own
    /// rendering fields (e.g. `active_model_identity`) in sync with the event
    /// and forwards the rest to `AppState`.
    pub fn reduce_ui_state_event(&mut self, event: UiStateEvent) {
        if let UiStateEvent::SetActiveModelIdentity(identity) = &event {
            self.active_model_identity = identity.clone();
        }
        self.state.reduce_ui_state_event(event);
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

    pub fn set_mcp_visible_tool_count_by_server_name(&mut self, server_name: &str, count: usize) {
        self.state
            .set_mcp_visible_tool_count_by_server_name(server_name, count);
    }

    pub fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        server_name: &str,
        names: Vec<String>,
    ) {
        self.state
            .set_mcp_visible_tool_names_by_server_name(server_name, names);
    }

    pub fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.state.set_context_window_max_tokens(max_tokens);
    }

    pub fn set_model_picker_options(&mut self, options: Vec<ModelPickerOption>) {
        self.state.set_model_picker_options(options);
    }

    pub fn set_agent_picker_options(
        &mut self,
        options: Vec<nu_agent_core::protocol::picker::AgentPickerOption>,
    ) {
        self.state.set_agent_picker_options(options);
    }

    pub fn set_session_picker_options(&mut self, options: Vec<crate::state::SessionPickerOption>) {
        self.state.set_session_picker_options(options);
    }

    pub fn set_active_agent_identity(&mut self, name: &str) {
        self.state.set_active_agent_identity(name);
    }

    pub fn set_active_persona_icon(&mut self, icon: Option<String>) {
        self.state.active_persona_icon = icon;
    }

    pub fn set_agent_cycle_names(&mut self, names: Vec<String>) {
        self.state.agent_cycle_names = names;
    }

    pub(crate) fn set_repo_branch_caller_cwd(&mut self, caller_cwd: Option<std::path::PathBuf>) {
        self.repo_branch_tracker = Some(status::RepoBranchTracker::from_caller_cwd(caller_cwd));
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub fn fatal_error(&self) -> Option<&str> {
        self.fatal_error.as_deref()
    }

    pub fn event_sender(
        &self,
    ) -> &tokio::sync::mpsc::Sender<nu_agent_core::orchestrator::OrchestratorEvent> {
        &self.state.event_tx
    }

    pub fn take_cancel_requested(&self) -> bool {
        self.cancel_controller.take_cancel_requested()
    }

    pub fn enqueue_ui_event(&mut self, event: UiEvent) {
        log::trace!("tui: enqueue_ui_event {:?}", std::mem::discriminant(&event));
        self.transport.enqueue_ui_event(event);
    }

    pub fn poll_terminal_event(&mut self, event_source: &mut impl TerminalEventSource) {
        // Pick up any restored input text from cancelled prompts before
        // processing the next event.
        if let Some(text) = self.state.restored_input_text.take() {
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            let last_line = lines.len().saturating_sub(1) as u16;
            let last_col = lines.last().map(|l| l.len()).unwrap_or(0) as u16;
            self.textarea = ratatui_textarea::TextArea::new(lines);
            self.textarea
                .move_cursor(ratatui_textarea::CursorMove::Jump(last_line, last_col));
            self.mark_render_needed();
        }

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
        self.handle_terminal_event(event);
    }

    /// Process a single terminal event received from the input source: route
    /// insert-mode characters/paste into the textarea, and dispatch everything
    /// else (submit, quit, navigation, pickers) through the reducer. Shared by
    /// the blocking terminal poll path and the async render loop.
    pub fn handle_terminal_event(&mut self, event: TerminalEvent) {
        if let TerminalEvent::Key(TerminalKey::Esc) = event
            && self.state.phase == crate::state::UiPhase::Idle
            && !self.state.command_palette_open
            && self.state.info_panel.is_none()
        {
            self.state.status_line = "Esc pressed. Press Ctrl+C to quit.".to_string();
        }

        if let TerminalEvent::Resize(_) = event {
            self.state.clear_assistant_projection_cache();
            self.state.entry_visual_info_dirty = true;
            self.recompute_layout_for_current_input();
        }

        if let TerminalEvent::Paste(text) = &event
            && self.state.input_mode == InputMode::Insert
            && !self.state.command_palette_open
            && self.state.info_panel.is_none()
        {
            self.textarea.insert_str(text.as_str());
            let buffer = self.textarea.lines().join("\n");
            self.state.check_inline_slash(&buffer);
            self.mark_render_needed();
            self.recompute_layout_for_current_input();
            self.flush_clipboard_request();
            self.quit_requested |= self.state.quit_requested;
            self.sync_transcript_viewport_lines_with_layout();
            self.mark_render_needed();
            return;
        }

        let changed = if let TerminalEvent::Key(key) = event
            && self.state.input_mode == InputMode::Insert
            && !self.state.command_palette_open
            && self.state.info_panel.is_none()
        {
            let handled = self.handle_insert_mode_key(key);
            if handled {
                true
            } else {
                // Fall through to dispatch for keys not handled by TextArea
                // (CtrlC, Esc, CtrlP, CtrlN, Tab, etc.)
                dispatch_terminal_event(
                    &mut self.state,
                    &TerminalEvent::Key(key),
                    Some(&self.cancel_controller),
                )
            }
        } else {
            dispatch_terminal_event(&mut self.state, &event, Some(&self.cancel_controller))
        };

        if changed {
            self.mark_render_needed();
        }
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

    /// Handle a key event in insert mode.
    /// Returns `true` if the event was consumed, `false` if it should fall through
    /// to the normal dispatch path.
    fn handle_insert_mode_key(&mut self, key: TerminalKey) -> bool {
        // Command palette open? Route to palette, not TextArea.
        if self.state.command_palette_open {
            return false;
        }

        // Picker open? Route to dispatch so rewrite_action handles picker keys
        // (query filtering, navigation, selection, close).
        if self.state.model_picker_open {
            return false;
        }
        if self.state.agent_picker_open {
            return false;
        }
        if self.state.session_picker_open {
            return false;
        }

        // Info panel open? Route to dispatch so rewrite_action handles
        // info panel keys (j/k scrolling, Esc to close, etc.).
        if self.state.info_panel.is_some() {
            return false;
        }

        // Permission prompt active? Don't mutate textarea.
        if self.state.has_permission_prompt() {
            return false;
        }

        // Inline slash open? Handle keys directly (navigation, accept, close).
        if self.state.inline_slash_open {
            return self.handle_inline_slash_key(key);
        }

        match key {
            // Submit — read textarea, clear it, dispatch submit
            TerminalKey::Enter => {
                let text = self.textarea.lines().join("\n");
                self.textarea = ratatui_textarea::TextArea::default();
                self.state.pending_submit_text = Some(text);
                let changed = dispatch_terminal_event(
                    &mut self.state,
                    &TerminalEvent::Key(TerminalKey::Enter),
                    Some(&self.cancel_controller),
                );
                self.state.check_inline_slash("");
                self.mark_render_needed();
                changed
            }
            // Esc — let dispatch handle
            TerminalKey::Esc => false,
            // History — navigate within multiline textarea or history
            TerminalKey::Up => {
                let ratatui_textarea::DataCursor(row, _) = self.textarea.cursor();
                if row > 0 {
                    self.textarea.input(ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Up,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    });
                    self.mark_render_needed();
                    true
                } else {
                    self.handle_history_up()
                }
            }
            TerminalKey::Down => {
                let ratatui_textarea::DataCursor(row, _) = self.textarea.cursor();
                let line_count = self.textarea.lines().len();
                if row < line_count.saturating_sub(1) {
                    self.textarea.input(ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Down,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    });
                    self.mark_render_needed();
                    true
                } else {
                    self.handle_history_down()
                }
            }
            // Navigation keys — route directly to TextArea
            TerminalKey::Left | TerminalKey::Right | TerminalKey::Home | TerminalKey::End => {
                let input = match key {
                    TerminalKey::Left => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Left,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                    TerminalKey::Right => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Right,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                    TerminalKey::Home => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Home,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                    TerminalKey::End => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::End,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                    _ => unreachable!(),
                };
                self.textarea.input(input);
                let buffer = self.textarea.lines().join("\n");
                self.state.check_inline_slash(&buffer);
                self.mark_render_needed();
                true
            }
            // Backspace/Delete — route directly to TextArea
            TerminalKey::Backspace | TerminalKey::Delete => {
                let input = match key {
                    TerminalKey::Backspace => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Backspace,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                    TerminalKey::Delete => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Delete,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                    _ => unreachable!(),
                };
                self.textarea.input(input);
                let buffer = self.textarea.lines().join("\n");
                self.state.check_inline_slash(&buffer);
                self.mark_render_needed();
                true
            }
            // AltEnter/ShiftEnter — insert newline directly
            TerminalKey::AltEnter | TerminalKey::ShiftEnter => {
                self.textarea.insert_newline();
                let buffer = self.textarea.lines().join("\n");
                self.state.check_inline_slash(&buffer);
                self.mark_render_needed();
                true
            }
            // All other keys — let dispatch handle them
            TerminalKey::PageUp
            | TerminalKey::PageDown
            | TerminalKey::CtrlU
            | TerminalKey::CtrlD
            | TerminalKey::CtrlP
            | TerminalKey::CtrlN
            | TerminalKey::Tab
            | TerminalKey::BackTab => false,
            TerminalKey::CtrlC => {
                let text = self.textarea.lines().join("\n");
                if !text.is_empty() && self.state.phase == crate::state::UiPhase::Idle {
                    self.textarea = ratatui_textarea::TextArea::default();
                    self.state.check_inline_slash("");
                    self.mark_render_needed();
                    true
                } else {
                    false
                }
            }
            // Char keys — map to UserAction, run through rewrite_action, then route
            TerminalKey::Char(ch) => {
                let mapped_action = UserAction::InsertChar(ch);
                let (rewritten, force_changed) = rewrite_action(&mut self.state, mapped_action);
                match rewritten {
                    UserAction::InsertChar(ch) => {
                        if self.state.input_locked {
                            self.mark_render_needed();
                            true
                        } else {
                            self.textarea.input(ratatui_textarea::Input {
                                key: ratatui_textarea::Key::Char(ch),
                                ctrl: false,
                                alt: false,
                                shift: false,
                            });
                            let buffer = self.textarea.lines().join("\n");
                            self.state.check_inline_slash(&buffer);
                            self.mark_render_needed();
                            true
                        }
                    }
                    UserAction::EnterNormalModeFromChord => {
                        // j/k chord triggered normal mode exit
                        // Remove the 'j' that was already inserted by TextArea
                        if !self.state.input_locked {
                            self.textarea.input(ratatui_textarea::Input {
                                key: ratatui_textarea::Key::Backspace,
                                ctrl: false,
                                alt: false,
                                shift: false,
                            });
                        }
                        let _changed = reduce_with_cancel_controller(
                            &mut self.state,
                            ReducerInput::User(UserAction::EnterNormalModeFromChord),
                            Some(&self.cancel_controller),
                        );
                        self.mark_render_needed();
                        true
                    }
                    UserAction::Noop => {
                        // rewrite_action modified state (e.g. insert_exit_pending_j set)
                        if force_changed {
                            self.mark_render_needed();
                        }
                        true
                    }
                    _ => {
                        // Some other action — let dispatch handle it
                        false
                    }
                }
            }
        }
    }

    /// Handle keys when inline slash suggestions are open.
    /// Routes characters to TextArea, navigation to suggestion cycling,
    /// Enter/Esc to accept/close, and Backspace to delete + re-check.
    fn handle_inline_slash_key(&mut self, key: TerminalKey) -> bool {
        match key {
            TerminalKey::Char(ch) => {
                let (rewritten, _) = rewrite_action(&mut self.state, UserAction::InsertChar(ch));
                match rewritten {
                    UserAction::InsertChar(ch) => {
                        self.textarea.input(ratatui_textarea::Input {
                            key: ratatui_textarea::Key::Char(ch),
                            ctrl: false,
                            alt: false,
                            shift: false,
                        });
                        let buffer = self.textarea.lines().join("\n");
                        self.state.check_inline_slash(&buffer);
                        self.mark_render_needed();
                        true
                    }
                    UserAction::InlineSlashAccept => {
                        let _changed = reduce_with_cancel_controller(
                            &mut self.state,
                            ReducerInput::User(UserAction::InlineSlashAccept),
                            Some(&self.cancel_controller),
                        );
                        self.textarea = ratatui_textarea::TextArea::default();
                        self.state.check_inline_slash("");
                        self.mark_render_needed();
                        true
                    }
                    UserAction::InlineSlashClose => {
                        self.state.check_inline_slash("");
                        self.mark_render_needed();
                        true
                    }
                    _ => false,
                }
            }
            TerminalKey::Up | TerminalKey::BackTab => {
                self.state.inline_slash_move_up();
                self.mark_render_needed();
                true
            }
            TerminalKey::Down => {
                self.state.inline_slash_move_down();
                self.mark_render_needed();
                true
            }
            TerminalKey::Tab | TerminalKey::Enter => {
                let changed = reduce_with_cancel_controller(
                    &mut self.state,
                    ReducerInput::User(UserAction::InlineSlashAccept),
                    Some(&self.cancel_controller),
                );
                self.textarea = ratatui_textarea::TextArea::default();
                self.state.check_inline_slash("");
                self.mark_render_needed();
                changed
            }
            TerminalKey::Esc => {
                self.state.check_inline_slash("");
                self.mark_render_needed();
                true
            }
            TerminalKey::Backspace => {
                self.textarea.input(ratatui_textarea::Input {
                    key: ratatui_textarea::Key::Backspace,
                    ctrl: false,
                    alt: false,
                    shift: false,
                });
                let buffer = self.textarea.lines().join("\n");
                self.state.check_inline_slash(&buffer);
                self.mark_render_needed();
                true
            }
            _ => false,
        }
    }

    /// Navigate up in input history.
    /// Saves current textarea content, then loads history item into textarea.
    fn handle_history_up(&mut self) -> bool {
        let current = self.textarea.lines().join("\n");
        if let Some(text) = self.state.history_up(&current) {
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            self.textarea = ratatui_textarea::TextArea::new(lines);
            self.mark_render_needed();
        }
        true
    }

    /// Navigate down in input history.
    /// Loads next history item or restores saved draft into textarea.
    fn handle_history_down(&mut self) -> bool {
        if let Some(text) = self.state.history_down() {
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            self.textarea = ratatui_textarea::TextArea::new(lines);
            self.mark_render_needed();
        }
        true
    }

    fn recompute_layout_for_current_input(&mut self) {
        let line_count = self.textarea.lines().len() as u16;
        self.input_height = line_count.clamp(INPUT_MIN_HEIGHT, INPUT_MAX_HEIGHT);
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

        // Apply any queued theme switch requests
        while let Some(name) = self.take_next_theme_switch_request() {
            if let Some(theme_name) = ThemeName::from_name(&name) {
                self.theme_name = theme_name;
                self.theme = theme_name.resolve();
                self.state.theme = self.theme.clone();
                self.state.clear_assistant_projection_cache();
                self.state.entry_visual_info_dirty = true;
            }
        }

        self.mark_render_needed();
    }

    pub(crate) fn mark_render_needed(&mut self) {
        self.render_needed = true;
    }

    pub(crate) fn render_if_needed(
        &mut self,
        live: &mut Option<LiveTerminalUi>,
    ) -> Result<(), String> {
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

    /// Apply selection highlight to buffer cells for the given visual row range.
    fn apply_selection_highlight(
        buffer: &mut ratatui::buffer::Buffer,
        area: ratatui::layout::Rect,
        sel_start: usize,
        sel_end: usize,
        effective_offset: usize,
        viewport_height: usize,
        selection_style: ratatui::style::Style,
    ) {
        for vis_row in sel_start..=sel_end {
            if vis_row >= effective_offset && vis_row < effective_offset + viewport_height {
                let row_y = (vis_row - effective_offset) as u16;
                let row_screen_y = area.y + row_y;
                for x in area.x..area.x + area.width {
                    if let Some(cell) =
                        buffer.cell_mut(ratatui::layout::Position { x, y: row_screen_y })
                    {
                        cell.set_style(selection_style);
                    }
                }
            }
        }
    }

    pub(crate) fn render_frame(&mut self, live: &mut Option<LiveTerminalUi>) -> Result<(), String> {
        let Some(live) = live.as_mut() else {
            return Ok(());
        };

        // Capture values needed inside the FnOnce draw closure (avoid borrowing self mutably)
        let transcript_following_tail = self.state.transcript_following_tail;
        let transcript_scroll_offset = self.state.transcript_scroll_offset;

        // Smuggle the resolved effective_offset out of the FnOnce draw closure so we can
        // write it back to transcript_scroll_offset after draw() returns. This ensures that
        // when following_tail is true, transcript_scroll_offset reflects the actual rendered
        // position — otherwise the first scroll-up would jump to offset 0.
        let mut rendered_scroll_offset: Option<usize> = None;

        live.terminal
            .draw(|frame| {
                let area = frame.area();
                let has_side = self.side_pane_visible.unwrap_or(false);
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
                let side_margin = if main.width >= 8 { MAIN_SIDE_MARGIN } else { 0 };
                let content_main = main.inner(Margin {
                    vertical: 0,
                    horizontal: side_margin,
                });
                let queue_count = self.state.pending_prompt_count() as u16;
                let queue_h = queue_count + if queue_count > 0 { 1 } else { 0 };
                let available_inner_w = content_main.width.saturating_sub(4) as usize;
                let pre_right_width = {
                    let rc = status_right_content(
                        self.repo_branch_tracker.as_ref().and_then(|t| t.branch()),
                        self.repo_branch_tracker
                            .as_ref()
                            .and_then(|t| t.caller_cwd()),
                        &self.theme,
                    );
                    rc.map(|line| {
                        line.spans
                            .iter()
                            .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
                            .sum::<usize>()
                    })
                    .unwrap_or(0)
                };
                let pre_left_width = {
                    let probe = status_left_content(
                        &self.active_model_identity,
                        None,
                        &self.state,
                        &self.theme,
                        available_inner_w,
                    );
                    probe
                        .spans
                        .iter()
                        .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
                        .sum::<usize>()
                };
                let status_h = compute_status_h(available_inner_w, pre_left_width, pre_right_width);
                let bottom_box_h = compute_bottom_box_height(queue_h, self.input_height, status_h);
                let vertical = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(0),
                        Constraint::Fill(1),
                        Constraint::Length(bottom_box_h),
                    ])
                    .split(content_main);
                // vertical[0]=unused [1]=transcript [2]=entire bottom box

                let transcript_content_area = vertical[1];
                let transcript_list_area = Rect {
                    width: transcript_content_area.width.saturating_sub(2),
                    ..transcript_content_area
                };

                if vertical[1].height > 0 {
                    self.render_transcript_pane(
                        frame,
                        transcript_content_area,
                        transcript_list_area,
                        transcript_following_tail,
                        transcript_scroll_offset,
                        &mut rendered_scroll_offset,
                    );
                }

                let now_millis = current_time_millis();
                self.render_bottom_box(
                    frame,
                    vertical[2],
                    queue_h,
                    self.input_height,
                    status_h,
                    now_millis,
                );

                if has_side {
                    let side = horizontal[1];
                    let side_widget = Paragraph::new(Line::from("Events pane reserved"))
                        .block(Block::default().borders(Borders::ALL).title("Events"));
                    frame.render_widget(side_widget, side);
                }

                if self.state.command_palette_open {
                    self.render_command_palette(frame, area);
                }

                if self.state.info_panel.is_some() {
                    self.render_info_panel(frame, area);
                }

                if self.state.model_picker_open {
                    self.render_model_picker(frame, area);
                }

                if self.state.agent_picker_open {
                    self.render_agent_picker(frame, area);
                }

                if self.state.session_picker_open {
                    self.render_session_picker(frame, area);
                }

                if self.state.theme_picker_open {
                    self.render_theme_picker(frame, area);
                }
            })
            .map_err(|err| format!("TUI render failed: {err}"))?;

        // Write back the resolved scroll offset so that transcript_scroll_offset always
        // reflects the actual rendered position. Without this, when following_tail is true
        // transcript_scroll_offset stays at 0 and the first scroll-up key jumps to the top.
        if let Some(offset) = rendered_scroll_offset {
            self.state.transcript_scroll_offset = offset;
        }

        let cursor_style = self.state.input_mode.cursor_style();
        let _ = crossterm::execute!(std::io::stdout(), cursor_style);

        Ok(())
    }
}

fn compute_status_h(available_inner_w: usize, left_width: usize, right_width: usize) -> u16 {
    if right_width == 0 || left_width + right_width <= available_inner_w {
        1
    } else {
        2
    }
}
fn compute_bottom_box_height(queue_content: u16, input_content: u16, status_content: u16) -> u16 {
    let borders = 2u16;
    let dividers = 1u16;
    borders + dividers + queue_content + input_content + status_content
}
