//! The TUI runtime coordinator: owns the application state, input transport,
//! and render bookkeeping, and exposes the operations the TUI event loop and
//! renderer invoke.

use std::time::{Duration, Instant};

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::layout::{Margin, Rect};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::interaction::{
    cancel::CancelController,
    dispatch::{dispatch_terminal_event, rewrite_action},
    input::{TerminalEvent, TerminalKey},
    reducer::{ReducerInput, UserAction, reduce_with_cancel_controller},
};
use crate::platform::transport::{TransportItem, TuiTransport};
use crate::rendering::{
    layout::{INPUT_MAX_HEIGHT, INPUT_MIN_HEIGHT, MAIN_SIDE_MARGIN},
    theme::TuiTheme,
};
use crate::runtime::layout::{compute_bottom_box_height, compute_status_h};
use crate::runtime::render::frame::current_time_millis;
use crate::runtime::status::{
    RepoBranchTracker, availability_label, status_left_content, status_right_content,
};
use crate::state::{
    AppState, InputMode, McpServerState, McpServerUsabilityState, PickerRenderKind, SubmitAction,
};
use nu_agent_core::bus::{CompactionEvent, LlmEvent, ToolEvent, TurnEvent, WarningEvent};
use nu_agent_core::orchestrator::UiStateEvent;
use nu_agent_core::protocol::contracts::UiMessageSnapshot;
use nu_agent_core::protocol::event::UiEvent;
use nu_agent_core::protocol::skills::DiscoverableSkill as ProtocolDiscoverableSkill;
use nu_agent_core::tools::mcp::runtime::McpServerLifecycle;

/// The runtime coordinator that owns the TUI application state, the input
/// transport, and render bookkeeping for the interactive render loop.
#[derive(Debug)]
pub struct RuntimeCoordinator {
    pub(crate) state: AppState,
    transport: TuiTransport,
    pub(crate) cancel_controller: CancelController,
    input_height: u16,
    side_pane_visible: Option<bool>,
    pub(crate) quit_requested: bool,
    pub(crate) fatal_error: Option<String>,
    pub(crate) input_backend_status: String,
    pub(crate) last_input_poll_status: String,
    pub(crate) last_input_error: Option<String>,
    input_watchdog_started_at: Instant,
    input_watchdog_timeout: Duration,
    pub(crate) repo_branch_tracker: Option<RepoBranchTracker>,
    pub(crate) theme: TuiTheme,
    pub(crate) render_needed: bool,
    pub(crate) last_render_at: Instant,
    pub(crate) textarea: ratatui_textarea::TextArea<'static>,
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
        self.state.transcript.hydrate_from_messages(
            messages,
            last_total_tokens,
            &mut self.state.status,
            &mut self.state.tool,
            &mut self.state.compaction,
        );
    }

    pub(crate) fn new_with_watchdog(
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
            transport: TuiTransport::default(),
            cancel_controller: CancelController::default(),
            input_height: INPUT_MIN_HEIGHT,
            side_pane_visible,
            quit_requested: false,
            fatal_error: None,
            input_backend_status: "unknown".to_string(),
            last_input_poll_status: "waiting for input poll".to_string(),
            last_input_error: None,
            input_watchdog_started_at: Instant::now(),
            input_watchdog_timeout,
            repo_branch_tracker: None,
            theme: theme.clone(),
            render_needed: true,
            last_render_at: Instant::now() - Duration::from_millis(100),
            textarea: ratatui_textarea::TextArea::default(),
        };
        coordinator.state.theme = theme;
        coordinator.sync_transcript_viewport_lines_with_layout();
        coordinator
    }

    /// Consume a UI-state event from the bus. Status-owned events are handled
    /// by `StatusState` first; the rest fall through to `AppState`.
    pub fn reduce_ui_state_event(&mut self, event: UiStateEvent) {
        if self.state.status.reduce_ui_state_event(event.clone()) {
            return;
        }
        self.state.reduce_ui_state_event(event);
    }

    /// Consume a tool lifecycle event from the bus.
    pub fn reduce_tool_event(&mut self, event: ToolEvent) -> bool {
        crate::state::dispatch_tool_event(&mut self.state, event)
    }

    /// Consume an LLM lifecycle event from the bus.
    pub fn reduce_llm_event(&mut self, event: LlmEvent) -> bool {
        crate::state::dispatch_llm_event(&mut self.state, event)
    }

    /// Consume a compaction lifecycle event from the bus. Returns false for
    /// events the TUI does not render (`CompactionEvent::Requested`).
    pub fn reduce_compaction_event(&mut self, event: CompactionEvent) -> bool {
        crate::state::dispatch_compaction_event(&mut self.state, event)
    }

    /// Consume a turn lifecycle event from the bus. Returns false for events
    /// the TUI does not render (`TurnEvent::Started` / `TaskCompleted`).
    pub fn reduce_turn_event(&mut self, event: TurnEvent) -> bool {
        crate::state::dispatch_turn_event(&mut self.state, event)
    }

    /// Dispatch a protocol `UiEvent` to the domain reducers. Both transport
    /// event paths (`drain_transport` and the warning fallback) converge here.
    pub(crate) fn reduce_ui_event(&mut self, event: UiEvent) -> bool {
        crate::interaction::reducer::dispatch_ui_event(&mut self.state, event)
    }

    /// Consume a warning event from the bus. `StatusState` handles plain
    /// messages (status line only); everything else falls through to the
    /// domain dispatch via the existing `WarningEvent -> UiEvent` conversion
    /// (TurnError keeps its spacer + error line + finalize behavior).
    pub fn reduce_warning_event(&mut self, event: WarningEvent) -> bool {
        if self.state.status.reduce_warning_event(event.clone()) {
            return true;
        }
        match Option::<UiEvent>::from(event) {
            Some(ui_event) => self.reduce_ui_event(ui_event),
            None => false,
        }
    }

    pub(crate) fn set_mcp_lifecycle_projection(&mut self, projection: Vec<McpServerLifecycle>) {
        let servers = projection
            .into_iter()
            .map(|server| {
                let name = server.name;
                self.state.status.set_mcp_visible_tool_count_by_server_name(
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
        self.state.status.set_mcp_servers(servers);
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
        self.state.status.set_discoverable_skills(mapped);
    }

    pub(crate) fn mark_skills_discovery_failed(&mut self) {
        self.state.status.mark_skills_discovery_failed();
    }

    pub(crate) fn set_llm_visible_mcp_tool_count(&mut self, count: usize) {
        self.state.status.set_llm_visible_mcp_tool_count(count);
    }

    pub fn set_mcp_visible_tool_count_by_server_name(&mut self, server_name: &str, count: usize) {
        self.state
            .status
            .set_mcp_visible_tool_count_by_server_name(server_name, count);
    }

    pub fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        server_name: &str,
        names: Vec<String>,
    ) {
        self.state
            .status
            .set_mcp_visible_tool_names_by_server_name(server_name, names);
    }

    pub fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.state.status.set_context_window_max_tokens(max_tokens);
    }

    pub fn set_active_agent_identity(&mut self, name: &str) {
        self.state.set_active_agent_identity(name);
    }

    pub fn set_active_persona_icon(&mut self, icon: Option<String>) {
        self.state.status.active_persona_icon = icon;
    }

    pub fn set_agent_cycle_names(&mut self, names: Vec<String>) {
        self.state.status.agent_cycle_names = names;
    }

    pub(crate) fn set_repo_branch_caller_cwd(&mut self, caller_cwd: Option<std::path::PathBuf>) {
        self.repo_branch_tracker = Some(RepoBranchTracker::from_caller_cwd(caller_cwd));
    }

    /// The git ref files that, when changed, indicate a branch switch. The
    /// render loop subscribes a filesystem watcher to these.
    pub(crate) fn repo_branch_watch_targets(&self) -> Vec<std::path::PathBuf> {
        self.repo_branch_tracker
            .as_ref()
            .map(|t| t.watch_targets().to_vec())
            .unwrap_or_default()
    }

    /// Re-read the current git branch after a filesystem change event.
    pub(crate) fn refresh_repo_branch(&mut self) {
        if let Some(tracker) = self.repo_branch_tracker.as_mut() {
            tracker.refresh();
        }
        self.mark_render_needed();
    }

    /// The current git branch shown in the status bar, if any.
    pub(crate) fn repo_branch(&self) -> Option<&str> {
        self.repo_branch_tracker.as_ref().and_then(|t| t.branch())
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

    pub fn poll_terminal_event(
        &mut self,
        event_source: &mut impl crate::runtime::TerminalEventSource,
    ) {
        // Pick up any restored input text from cancelled prompts before
        // processing the next event.
        if let Some(text) = self.state.input.restored_input_text.take() {
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            let last_line = lines.len().saturating_sub(1) as u16;
            let last_col = lines.last().map(|l| l.len()).unwrap_or(0) as u16;
            self.textarea = ratatui_textarea::TextArea::new(lines);
            self.textarea
                .move_cursor(ratatui_textarea::CursorMove::Jump(last_line, last_col));
            self.mark_render_needed();
        }

        if let Some(tracker) = self.repo_branch_tracker.as_mut() {
            tracker.refresh();
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
                self.state.status.status_line = format!("Terminal input error: {error}");
                self.fatal_error = Some(self.state.status.status_line.clone());
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
            && self.state.picker.active().is_none()
            && self.state.info_panel.is_none()
        {
            self.state.status.status_line = "Esc pressed. Press Ctrl+C to quit.".to_string();
        }

        if let TerminalEvent::Resize(_) = event {
            self.state.transcript.clear_assistant_projection_cache();
            self.state.transcript.visual_info_dirty = true;
            self.recompute_layout_for_current_input();
        }

        if let TerminalEvent::Paste(text) = &event
            && self.state.input.mode == InputMode::Insert
            && (self.state.picker.active().is_none()
                || self.state.picker.render_kind() == Some(PickerRenderKind::InlineSlash))
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
            && self.state.input.mode == InputMode::Insert
            && (self.state.picker.active().is_none()
                || self.state.picker.render_kind() == Some(PickerRenderKind::InlineSlash))
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
        // Info panel open? Route to dispatch so rewrite_action handles
        // info panel keys (j/k scrolling, Esc to close, etc.).
        if self.state.info_panel.is_some() {
            return false;
        }

        // Permission prompt active? Don't mutate textarea.
        if self.state.permission.has_prompt() {
            return false;
        }

        // Inline slash open? Handle keys directly (navigation, accept, close).
        if self.state.picker.render_kind() == Some(PickerRenderKind::InlineSlash) {
            return self.handle_inline_slash_key(key);
        }

        match key {
            // Submit — read textarea, clear it, dispatch submit
            TerminalKey::Enter => {
                let text = self.textarea.lines().join("\n");
                self.textarea = ratatui_textarea::TextArea::default();
                self.state.input.pending_submit_text = Some(text);
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
                    // Unreachable: the outer guard restricts to the four arms above.
                    _ => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Null,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
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
                    // Unreachable: the outer guard restricts to the two arms above.
                    _ => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Null,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
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
                        // rewrite_action modified input chord state (e.g. the
                        // insert-exit chord armed)
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
                    UserAction::PickerSubmit(SubmitAction::SlashAccept) => {
                        let _changed = reduce_with_cancel_controller(
                            &mut self.state,
                            ReducerInput::User(UserAction::PickerSubmit(SubmitAction::SlashAccept)),
                            Some(&self.cancel_controller),
                        );
                        self.textarea = ratatui_textarea::TextArea::default();
                        self.state.check_inline_slash("");
                        self.mark_render_needed();
                        true
                    }
                    _ => false,
                }
            }
            TerminalKey::Up | TerminalKey::BackTab => {
                if let Some(s) = self.state.picker.active_state_mut() {
                    s.move_up();
                }
                self.mark_render_needed();
                true
            }
            TerminalKey::Down => {
                if let Some(s) = self.state.picker.active_state_mut() {
                    s.move_down();
                }
                self.mark_render_needed();
                true
            }
            TerminalKey::Tab | TerminalKey::Enter => {
                let changed = reduce_with_cancel_controller(
                    &mut self.state,
                    ReducerInput::User(UserAction::PickerSubmit(SubmitAction::SlashAccept)),
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
    /// History navigation only applies while idle; the phase is
    /// orchestrator-owned, so the gate lives here, not in InputState.
    fn handle_history_up(&mut self) -> bool {
        if self.state.phase != crate::state::UiPhase::Idle {
            return true;
        }
        let current = self.textarea.lines().join("\n");
        let submitted = self.state.submitted_prompt_texts();
        if let Some(text) = self.state.input.history_up(&submitted, &current) {
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            self.textarea = ratatui_textarea::TextArea::new(lines);
            self.mark_render_needed();
        }
        true
    }

    /// Navigate down in input history.
    /// Loads next history item or restores saved draft into textarea.
    fn handle_history_down(&mut self) -> bool {
        if self.state.phase != crate::state::UiPhase::Idle {
            return true;
        }
        let submitted = self.state.submitted_prompt_texts();
        if let Some(text) = self.state.input.history_down(&submitted) {
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
        let Some(text) = self.state.input.take_clipboard_request() else {
            return;
        };

        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
            Ok(()) => {
                self.state.status.status_line = "Copied selection to clipboard.".to_string();
            }
            Err(error) => {
                self.state.status.status_line = format!("Clipboard copy failed: {error}");
            }
        }
    }

    fn update_input_diagnostics(&mut self, diagnostics: &crate::runtime::InputSourceDiagnostics) {
        let primary = availability_label(diagnostics.primary_available);
        let fallback = availability_label(diagnostics.fallback_available);
        self.input_backend_status = format!(
            "active={}, crossterm={}, /dev/tty={}",
            diagnostics.active_backend, primary, fallback
        );
        self.last_input_poll_status = diagnostics.last_poll_state.clone();
        self.last_input_error = diagnostics.last_error.clone();
    }

    fn maybe_trigger_input_watchdog(
        &mut self,
        diagnostics: &crate::runtime::InputSourceDiagnostics,
    ) {
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

    fn both_backends_unavailable(diagnostics: &crate::runtime::InputSourceDiagnostics) -> bool {
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

        self.state.status.status_line = message.clone();
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
                let TransportItem::Event(e) = item else {
                    // Guard above guarantees this is an Event; drop defensively.
                    continue;
                };
                pending_assistant = Some(*e);
                continue;
            }

            if matches!(
                &item,
                TransportItem::Event(e) if matches!(e.as_ref(), UiEvent::CompactionSummaryChunk { .. })
            ) {
                let TransportItem::Event(e) = item else {
                    // Guard above guarantees this is an Event; drop defensively.
                    continue;
                };
                pending_compaction = Some(*e);
                continue;
            }

            // Flush any pending coalesced events before processing a different event type
            // (preserves ordering: assistant text before tool events, etc.)
            if let Some(event) = pending_assistant.take() {
                self.reduce_ui_event(event);
            }
            if let Some(event) = pending_compaction.take() {
                self.reduce_ui_event(event);
            }

            // Process the current non-coalesceable event
            match item {
                TransportItem::User(action) => {
                    reduce_with_cancel_controller(
                        &mut self.state,
                        ReducerInput::User(action),
                        Some(&self.cancel_controller),
                    );
                }
                TransportItem::Event(event) => {
                    self.reduce_ui_event(*event);
                }
            }
        }

        // Flush remaining pending events
        if let Some(event) = pending_assistant.take() {
            self.reduce_ui_event(event);
        }
        if let Some(event) = pending_compaction.take() {
            self.reduce_ui_event(event);
        }

        self.mark_render_needed();
    }

    pub(crate) fn mark_render_needed(&mut self) {
        self.render_needed = true;
    }

    pub(crate) fn render_if_needed(
        &mut self,
        live: &mut Option<crate::runtime::LiveTerminalUi>,
    ) -> Result<(), String> {
        if !self.render_needed {
            return Ok(());
        }
        if self.last_render_at.elapsed() < Self::MIN_FRAME_INTERVAL {
            return Ok(());
        }
        self.render_needed = false;
        self.last_render_at = Instant::now();
        self.theme = self.state.theme.clone();
        self.render_frame(live)
    }

    /// Apply selection highlight to buffer cells for the given visual row range.
    pub(crate) fn apply_selection_highlight(
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

    pub(crate) fn render_frame(
        &mut self,
        live: &mut Option<crate::runtime::LiveTerminalUi>,
    ) -> Result<(), String> {
        let Some(live) = live.as_mut() else {
            return Ok(());
        };

        // Capture values needed inside the FnOnce draw closure (avoid borrowing self mutably)
        let transcript_following_tail = self.state.scroll.following_tail;
        let transcript_scroll_offset = self.state.scroll.scroll_offset;

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
                        self.repo_branch(),
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
                    let probe =
                        status_left_content(None, &self.state, &self.theme, available_inner_w);
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

                match self.state.picker.render_kind() {
                    Some(PickerRenderKind::CommandPalette) => {
                        self.render_command_palette(frame, area);
                    }
                    Some(PickerRenderKind::Model) => self.render_model_picker(frame, area),
                    Some(PickerRenderKind::Agent) => self.render_agent_picker(frame, area),
                    Some(PickerRenderKind::Session) => self.render_session_picker(frame, area),
                    Some(PickerRenderKind::Theme) => self.render_theme_picker(frame, area),
                    Some(PickerRenderKind::InlineSlash) | None => {}
                }

                if self.state.info_panel.is_some() {
                    self.render_info_panel(frame, area);
                }
            })
            .map_err(|err| format!("TUI render failed: {err}"))?;

        // Write back the resolved scroll offset so that scroll_offset always
        // reflects the actual rendered position. Without this, when following_tail is true
        // scroll_offset stays at 0 and the first scroll-up key jumps to the top.
        if let Some(offset) = rendered_scroll_offset {
            self.state.scroll.scroll_offset = offset;
        }

        let cursor_style = self.state.input.mode.cursor_style();
        let _ = crossterm::execute!(std::io::stdout(), cursor_style);

        Ok(())
    }
}
