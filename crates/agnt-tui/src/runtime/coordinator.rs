//! The TUI runtime coordinator: owns the application state, input transport,
//! and render bookkeeping, and exposes the operations the TUI event loop and
//! renderer invoke.

use std::time::{Duration, Instant};

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};

use crate::interaction::{
    cancel::CancelController,
    dispatch::{KeyLens, dispatch_terminal_event, rewrite_action},
    input::{TerminalEvent, TerminalKey},
};
use crate::platform::transport::{Source, TuiTransport};
use crate::rendering::{
    chrome::{Chrome, ContainerBorder},
    layout::{INPUT_MAX_HEIGHT, INPUT_MIN_HEIGHT, MAIN_SIDE_MARGIN, input_content_row_count},
    theme::{ThemeName, TuiTheme},
};
use crate::runtime::backend::FrameSync;
use crate::runtime::layout::{compute_bottom_box_height, compute_status_h};
use crate::runtime::render::bottom_box::{queued_prompt_entries, stacked_messages_height};
use crate::runtime::render::frame::current_time_millis;
use crate::runtime::render::transcript::PaintWindow;
use crate::runtime::status::{
    RepoBranchTracker, availability_label, status_left_content, status_right_content,
};
use crate::state::{
    AppState, InputMode, McpServerState, McpServerUsabilityState, PickerRenderKind,
};
use agnt_core::orchestrator::UiStateEvent;
use agnt_core::protocol::contracts::UiMessageSnapshot;
use agnt_core::protocol::events::Key;
use agnt_core::protocol::events::SystemEvent;
use agnt_core::protocol::skills::DiscoverableSkill as ProtocolDiscoverableSkill;
use agnt_core::tools::mcp::runtime::McpServerLifecycle;
use agnt_core::transcript::renderer::ItemStatus;

/// The runtime coordinator that owns the TUI application state, the input
/// transport, and render bookkeeping for the interactive render loop.
#[derive(Debug)]
pub struct RuntimeCoordinator {
    pub(crate) state: AppState,
    transport: TuiTransport,
    pub(crate) cancel_controller: CancelController,
    pub(crate) input_height: u16,
    last_input_content_width: usize,
    side_pane_visible: Option<bool>,
    pub(crate) quit_requested: bool,
    pub(crate) fatal_error: Option<String>,
    pub(crate) input_backend_status: String,
    pub(crate) last_input_poll_status: String,
    pub(crate) last_input_error: Option<String>,
    input_watchdog_started_at: Instant,
    input_watchdog_timeout: Duration,
    pub(crate) repo_branch_tracker: Option<RepoBranchTracker>,
    pub(crate) theme_name: ThemeName,
    pub(crate) theme: TuiTheme,
    pub(crate) render_needed: bool,
    pub(crate) last_render_at: Instant,
    /// The busy spinner's cadence step that last produced an animation frame.
    /// `None` means "not animating" — the next animated step renders at once.
    last_spinner_step: Option<u128>,
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

    /// Build an input textarea with word-or-glyph soft wrap enabled.
    pub(crate) fn new_textarea(lines: Vec<String>) -> ratatui_textarea::TextArea<'static> {
        let mut textarea = ratatui_textarea::TextArea::new(lines);
        textarea.set_wrap_mode(ratatui_textarea::WrapMode::WordOrGlyph);
        textarea
    }

    /// Replace the input textarea. The crate's scroll state lives on the
    /// `TextArea` itself and starts at the top of the new content.
    fn replace_textarea(&mut self, lines: Vec<String>) {
        self.textarea = Self::new_textarea(lines);
    }

    pub fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
        last_total_tokens: Option<u64>,
    ) {
        crate::state::hydrate_by_replay(&mut self.state, messages, last_total_tokens);
    }

    pub(crate) fn new_with_watchdog(
        columns: u16,
        _rows: u16,
        side_pane_visible: Option<bool>,
        input_watchdog_timeout: Duration,
    ) -> Self {
        let theme_name = ThemeName::default();
        let theme = theme_name.resolve();
        let (event_tx, _event_rx) =
            tokio::sync::mpsc::channel::<agnt_core::orchestrator::OrchestratorEvent>(256);
        let side_margin = if columns >= 8 { MAIN_SIDE_MARGIN } else { 0 };
        let last_input_content_width = columns.saturating_sub(2 * side_margin + 4) as usize;
        let mut coordinator = Self {
            state: AppState::new_with_sender(event_tx),
            transport: TuiTransport::default(),
            cancel_controller: CancelController::default(),
            input_height: INPUT_MIN_HEIGHT,
            last_input_content_width,
            side_pane_visible,
            quit_requested: false,
            fatal_error: None,
            input_backend_status: "unknown".to_string(),
            last_input_poll_status: "waiting for input poll".to_string(),
            last_input_error: None,
            input_watchdog_started_at: Instant::now(),
            input_watchdog_timeout,
            repo_branch_tracker: None,
            theme_name,
            theme: theme.clone(),
            render_needed: true,
            last_render_at: Instant::now() - Duration::from_millis(100),
            last_spinner_step: None,
            textarea: Self::new_textarea(Vec::new()),
        };
        coordinator.state.theme = theme;
        coordinator.sync_transcript_viewport_lines_with_layout();
        coordinator
    }

    /// Set the active theme by name, mirroring the `SwitchRequest::Theme` arm
    /// in `AppState::take_pending_events`: update the name, resolve the theme,
    /// sync `state.theme`.
    ///
    /// no height-index invalidation on theme switch — safe only while
    /// measurement is theme-blind (see task 4e532fb9's invariant docs).
    pub fn set_theme(&mut self, name: ThemeName) {
        self.theme_name = name;
        self.theme = name.resolve();
        self.state.theme = self.theme.clone();
        self.state.theme_name = name;
        self.mark_render_needed();
    }

    /// Consume a UI-state event from the bus. Status-owned events are handled
    /// by `StatusState` first; the rest fall through to `AppState`.
    pub fn reduce_ui_state_event(&mut self, event: UiStateEvent) {
        if self.state.status.reduce_ui_state_event(event.clone()) {
            return;
        }
        self.state.reduce_ui_state_event(event);
    }

    /// Consume a `SystemEvent`. This is the only reducer entry point for
    /// events: the bus arms and the transport drain both converge here.
    pub fn reduce_system_event(&mut self, event: SystemEvent) -> bool {
        crate::interaction::reducer::reduce(&mut self.state, event, Some(&self.cancel_controller))
    }

    pub(crate) fn set_mcp_lifecycle_projection(&mut self, projection: Vec<McpServerLifecycle>) {
        let servers = projection
            .into_iter()
            .map(|server| {
                let name = server.name;
                self.state
                    .status
                    .mcp
                    .set_mcp_visible_tool_count_by_server_name(
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
        self.state.status.mcp.set_mcp_servers(servers);
    }

    pub(crate) fn set_skills_projection(&mut self, skills: Vec<ProtocolDiscoverableSkill>) {
        let mapped = skills
            .into_iter()
            .map(|skill| crate::state::DiscoverableSkill {
                source_priority: skill.source.priority(),
                source: skill.source.label().to_string(),
                name: skill.name,
                description: skill.description,
            })
            .collect();
        self.state.status.mcp.set_discoverable_skills(mapped);
    }

    pub(crate) fn mark_skills_discovery_failed(&mut self) {
        self.state.status.mcp.mark_skills_discovery_failed();
    }

    pub(crate) fn set_llm_visible_mcp_tool_count(&mut self, count: usize) {
        self.state.status.mcp.set_llm_visible_mcp_tool_count(count);
    }

    pub fn set_mcp_visible_tool_count_by_server_name(&mut self, server_name: &str, count: usize) {
        self.state
            .status
            .mcp
            .set_mcp_visible_tool_count_by_server_name(server_name, count);
    }

    pub fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        server_name: &str,
        names: Vec<String>,
    ) {
        self.state
            .status
            .mcp
            .set_mcp_visible_tool_names_by_server_name(server_name, names);
    }

    pub fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.state
            .status
            .tokens
            .set_context_window_max_tokens(max_tokens);
    }

    pub fn set_active_agent_identity(&mut self, name: &str) {
        self.state.set_active_agent_identity(name);
    }

    pub fn set_active_persona_icon(&mut self, icon: Option<String>) {
        self.state.status.identity.active_persona_icon = icon;
    }

    pub fn set_agent_cycle_names(&mut self, names: Vec<String>) {
        self.state.status.identity.agent_cycle_names = names;
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
    ) -> &tokio::sync::mpsc::Sender<agnt_core::orchestrator::OrchestratorEvent> {
        &self.state.event_tx
    }

    pub fn take_cancel_requested(&self) -> bool {
        self.cancel_controller.take_cancel_requested()
    }

    pub fn enqueue_event(&mut self, event: SystemEvent) {
        log::trace!("tui: enqueue_event {:?}", std::mem::discriminant(&event));
        self.transport.enqueue_event(event);
    }

    pub fn poll_terminal_event(
        &mut self,
        event_source: &mut impl crate::runtime::TerminalEventSource,
    ) {
        // Pick up any restored input text from cancelled prompts before
        // processing the next event.
        self.pickup_restored_input_text();

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
                self.fatal_error = Some(format!("Terminal input error: {error}"));
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
        // Pick up any restored input text from cancelled prompts before
        // processing the next event.
        self.pickup_restored_input_text();

        if self.handle_ctrl_c_guard(&event) {
            return;
        }

        if let TerminalEvent::Resize(resize) = event {
            self.state.transcript.invalidate_height_index();
            let side_margin = if resize.columns >= 8 {
                MAIN_SIDE_MARGIN
            } else {
                0
            };
            self.last_input_content_width =
                resize.columns.saturating_sub(2 * side_margin + 4) as usize;
            self.recompute_layout_for_current_input();
            // The Resize reducer arm reports no change (it performs no
            // mutation), so the branch marks the layout change itself.
            self.mark_render_needed();
        }

        // A paste writes the textarea, so it obeys the same gates as typed
        // keys: the input lock drops it, and an open permission card owns the
        // input. A gated-off paste falls through to dispatch, which maps it to
        // no action and mutates nothing.
        if let TerminalEvent::Paste(text) = &event
            && self.state.input.mode == InputMode::Insert
            && (self.state.picker.active().is_none()
                || self.state.picker.render_kind() == Some(PickerRenderKind::InlineSlash))
            && self.state.info_panel.is_none()
            && !self.state.input_locked
            && !self.state.permission.has_prompt()
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
    }

    /// Global Ctrl-C quit guard. Returns `true` when the event was consumed by
    /// the guard and must not be routed further.
    ///
    /// Priority chain — each Ctrl-C press does exactly one thing:
    /// 1. permission prompt open → arm (the prompt keeps its own keys; Esc denies)
    /// 2. any other popup open → close it, no arm
    /// 3. no popup and non-empty draft → clear the draft, no arm
    /// 4. otherwise → arm
    ///
    /// The confirming press quits regardless of popups or draft state. Any
    /// other key, paste, or mouse event disarms; a resize leaves the armed
    /// state untouched.
    pub(crate) fn handle_ctrl_c_guard(&mut self, event: &TerminalEvent) -> bool {
        match event {
            TerminalEvent::Resize(_) => false,
            TerminalEvent::Paste(_) => {
                if self.state.disarm_quit_confirmation() {
                    self.mark_render_needed();
                }
                false
            }
            TerminalEvent::Mouse(_) => {
                if self.state.disarm_quit_confirmation() {
                    self.mark_render_needed();
                }
                false
            }
            TerminalEvent::Key(TerminalKey::CtrlC) => {
                self.handle_ctrl_c();
                true
            }
            TerminalEvent::Key(_) => {
                if self.state.disarm_quit_confirmation() {
                    self.mark_render_needed();
                }
                false
            }
        }
    }

    fn handle_ctrl_c(&mut self) {
        if self.state.quit_confirmation_armed() {
            self.state.disarm_quit_confirmation();
            self.state.quit_requested = true;
            self.quit_requested = true;
            if self.state.phase != crate::state::UiPhase::Idle {
                self.cancel_controller.request_cancel();
            }
            self.mark_render_needed();
            return;
        }

        if self.state.permission.has_prompt() {
            self.state.arm_quit_confirmation();
            self.mark_render_needed();
            return;
        }

        if self.state.picker.active().is_some() {
            self.state.picker.close();
            self.mark_render_needed();
            return;
        }

        if self.state.info_panel.is_some() {
            self.state.close_info_panel();
            self.mark_render_needed();
            return;
        }

        if !self.textarea.lines().join("\n").is_empty() {
            self.replace_textarea(Vec::new());
            self.state.check_inline_slash("");
            self.recompute_layout_for_current_input();
            self.mark_render_needed();
            return;
        }

        self.state.arm_quit_confirmation();
        self.mark_render_needed();
    }

    /// Apply any restored input text from cancelled prompts to the textarea.
    /// Idempotent via `take()`: safe to call from both the sync poll path and
    /// the async render loop's terminal arm.
    fn pickup_restored_input_text(&mut self) {
        if let Some(text) = self.state.input.restored_input_text.take() {
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            let last_line = lines.len().saturating_sub(1) as u16;
            let last_col = lines.last().map(|l| l.len()).unwrap_or(0) as u16;
            self.replace_textarea(lines);
            self.textarea
                .move_cursor(ratatui_textarea::CursorMove::Jump(last_line, last_col));
            self.mark_render_needed();
        }
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
                // Empty input while busy with queued prompts: cancel the running
                // turn so the existing drain path submits the queue.
                if text.is_empty()
                    && self.state.phase == crate::state::UiPhase::Busy
                    && self.state.pending_prompt_count() > 0
                {
                    self.cancel_controller.request_cancel();
                    self.mark_render_needed();
                    return true;
                }
                self.replace_textarea(Vec::new());
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
                if self.textarea.screen_cursor().row > 0 {
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
                let wrapped_row_count =
                    input_content_row_count(&self.textarea, self.last_input_content_width as u16);
                if self.textarea.screen_cursor().row + 1 < wrapped_row_count as usize {
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
            // All other keys — let dispatch handle them. CtrlC never reaches
            // here: the global quit guard intercepts it before routing.
            TerminalKey::PageUp
            | TerminalKey::PageDown
            | TerminalKey::CtrlU
            | TerminalKey::CtrlD
            | TerminalKey::CtrlP
            | TerminalKey::CtrlN
            | TerminalKey::CtrlJ
            | TerminalKey::CtrlK
            | TerminalKey::Tab
            | TerminalKey::BackTab
            | TerminalKey::CtrlC => false,
            // Char keys — the lens may consume them (panel, picker, the card),
            // the reducer's insert-exit chord may consume them, otherwise the
            // textarea takes them.
            TerminalKey::Char(ch) => {
                let chord_pending =
                    matches!(ch, 'j' | 'k') && self.state.input.insert_exit_pending_j();
                match rewrite_action(&mut self.state, Key::Char(ch)) {
                    KeyLens::Pass => {
                        let _ = self
                            .reduce_system_event(SystemEvent::KeyPressed { key: Key::Char(ch) });
                        if chord_pending {
                            // The chord left insert mode: drop the j/k the textarea
                            // already holds.
                            if !self.state.input_locked {
                                self.textarea.input(ratatui_textarea::Input {
                                    key: ratatui_textarea::Key::Backspace,
                                    ctrl: false,
                                    alt: false,
                                    shift: false,
                                });
                            }
                            self.mark_render_needed();
                            true
                        } else if self.state.input_locked {
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
                    KeyLens::Consumed(changed) => {
                        if changed {
                            self.mark_render_needed();
                        }
                        true
                    }
                    _ => false,
                }
            }
        }
    }

    /// Handle keys when inline slash suggestions are open.
    /// Routes characters to TextArea, navigation to suggestion cycling,
    /// Enter/Esc to accept/close, and Backspace to delete + re-check.
    fn handle_inline_slash_key(&mut self, key: TerminalKey) -> bool {
        match key {
            TerminalKey::Char(ch) => match rewrite_action(&mut self.state, Key::Char(ch)) {
                KeyLens::Pass => {
                    let _ =
                        self.reduce_system_event(SystemEvent::KeyPressed { key: Key::Char(ch) });
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
                KeyLens::Consumed(changed) => {
                    if changed {
                        self.mark_render_needed();
                    }
                    true
                }
                _ => false,
            },
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
                let submit_key = if key == TerminalKey::Tab {
                    Key::Tab
                } else {
                    Key::Enter
                };
                let changed = self.reduce_system_event(SystemEvent::KeyPressed { key: submit_key });
                self.replace_textarea(Vec::new());
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
            self.replace_textarea(lines);
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
            self.replace_textarea(lines);
            self.mark_render_needed();
        }
        true
    }

    fn recompute_layout_for_current_input(&mut self) {
        self.input_height =
            input_content_row_count(&self.textarea, self.last_input_content_width as u16)
                .clamp(INPUT_MIN_HEIGHT, INPUT_MAX_HEIGHT);
    }

    fn flush_clipboard_request(&mut self) {
        let Some(text) = self.state.input.take_clipboard_request() else {
            return;
        };

        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
            Ok(()) => {}
            Err(error) => {
                self.state
                    .status
                    .message
                    .set_message(format!("Clipboard copy failed: {error}"));
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

        self.fatal_error = Some(message);
        self.quit_requested = true;
        self.cancel_controller.request_cancel();
    }

    pub fn drain_transport(&mut self) {
        // Coalescing rule, stated here and nowhere else: at most the latest
        // `InferenceChunk` and the latest `CompactionSummaryChunk` per drain
        // cycle. A pending chunk is flushed before any other event so the
        // streamed text keeps its place in the order, and a newer chunk of the
        // same kind replaces the older one. Input events are never coalesced.
        let mut pending_inference: Option<SystemEvent> = None;
        let mut pending_compaction: Option<SystemEvent> = None;
        let mut changed = false;

        while let Some(item) = self.transport.poll_next() {
            if item.source == Source::Event {
                if matches!(&item.event, SystemEvent::InferenceChunk { .. }) {
                    pending_inference = Some(item.event);
                    continue;
                }
                if matches!(&item.event, SystemEvent::CompactionSummaryChunk { .. }) {
                    pending_compaction = Some(item.event);
                    continue;
                }
            }

            if let Some(event) = pending_inference.take() {
                changed |= self.reduce_system_event(event);
            }
            if let Some(event) = pending_compaction.take() {
                changed |= self.reduce_system_event(event);
            }

            changed |= self.reduce_system_event(item.event);
        }

        // Flush the tail of the stream.
        if let Some(event) = pending_inference.take() {
            changed |= self.reduce_system_event(event);
        }
        if let Some(event) = pending_compaction.take() {
            changed |= self.reduce_system_event(event);
        }

        if changed {
            self.mark_render_needed();
        }
    }

    pub(crate) fn mark_render_needed(&mut self) {
        self.render_needed = true;
    }

    pub(crate) fn has_active_animation(&self) -> bool {
        crate::runtime::status::model_activity_label(&self.state) == "busy"
            || self.state.transcript.entries().iter().any(|entry| {
                matches!(
                    entry,
                    agnt_core::transcript::entry::TranscriptEntry::Tool(tool)
                        if tool.status == ItemStatus::InProgress
                )
            })
    }

    /// Whether the busy spinner advanced to a new cadence step since the last
    /// animation frame — the only reason the render loop renders without an
    /// event. The spinner's own 150 ms step is the animation cadence, so a
    /// repeated step reports `false` and the loop emits no bytes: an idle loop
    /// and a stalled animation both stay silent.
    ///
    /// `now_millis` is the frame clock, passed in so the gate is testable
    /// without a wall-clock wait.
    pub(crate) fn spinner_cadence_advanced(&mut self, now_millis: u128) -> bool {
        let step = self
            .has_active_animation()
            .then_some(now_millis / crate::runtime::SPINNER_CADENCE_MS);
        if step == self.last_spinner_step {
            return false;
        }
        self.last_spinner_step = step;
        step.is_some()
    }

    pub(crate) fn expire_status_message_if_due(&mut self, now: Instant) -> bool {
        if self.state.status.message.expired(now) {
            self.state.status.message.clear();
            true
        } else {
            false
        }
    }

    pub(crate) fn has_pending_status_message(&self) -> bool {
        self.state.status.message.is_pending()
    }

    pub(crate) fn render_if_needed<B: ratatui::backend::Backend + FrameSync>(
        &mut self,
        live: &mut Option<&mut ratatui::Terminal<B>>,
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

    /// Apply the selection highlight to the buffer rows that host the selection's
    /// visual rows.
    ///
    /// The rows come from the same [`PaintWindow`] the paint used, so the
    /// highlight lands where the paint put those rows — the window's `top`
    /// padding included. The visible frame and the highlight cannot drift.
    pub(crate) fn apply_selection_highlight(
        buffer: &mut ratatui::buffer::Buffer,
        area: ratatui::layout::Rect,
        sel_start: usize,
        sel_end: usize,
        window: &PaintWindow<'_>,
        selection_style: ratatui::style::Style,
    ) {
        for vis_row in sel_start..=sel_end {
            if !window.contains(vis_row) {
                continue;
            }
            let Ok(buffer_row) = u16::try_from(window.buffer_row(vis_row)) else {
                continue;
            };
            let row_screen_y = area.y.saturating_add(buffer_row);
            for x in area.x..area.x + area.width {
                if let Some(cell) =
                    buffer.cell_mut(ratatui::layout::Position { x, y: row_screen_y })
                {
                    cell.set_style(selection_style);
                }
            }
        }
    }

    pub(crate) fn render_frame<B: ratatui::backend::Backend + FrameSync>(
        &mut self,
        live: &mut Option<&mut ratatui::Terminal<B>>,
    ) -> Result<(), String> {
        let Some(live) = live.as_mut() else {
            return Ok(());
        };
        let cursor_style = self.state.input.mode.cursor_style();

        // A synchronized-output block must never outlive its frame: the terminal
        // would keep buffering and stop repainting. So open the block, run every
        // phase, and only then report the first failure.
        let begin_sync = live.backend_mut().begin_sync().map_err(frame_sync_error);

        // Capture values needed inside the FnOnce draw closure (avoid borrowing self mutably)
        let transcript_following_tail = self.state.scroll.following_tail;
        let transcript_scroll_offset = self.state.scroll.scroll_offset;

        // Smuggle the resolved effective_offset out of the FnOnce draw closure so we can
        // write it back to transcript_scroll_offset after draw() returns. This ensures that
        // when following_tail is true, transcript_scroll_offset reflects the actual rendered
        // position — otherwise the first scroll-up would jump to offset 0.
        let mut rendered_scroll_offset: Option<usize> = None;

        let draw = live
            .draw(|frame| {
                let area = frame.area();
                frame.render_widget(
                    Block::default().style(Style::default().bg(self.theme.base)),
                    area,
                );
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
                self.last_input_content_width = content_main.width.saturating_sub(4) as usize;
                let available_inner_w = content_main.width.saturating_sub(2) as usize;
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
                let has_message = !self.state.status.message.status_line().is_empty();
                let status_h = compute_status_h(
                    available_inner_w,
                    pre_left_width,
                    pre_right_width,
                    has_message,
                );
                let stack_entries = queued_prompt_entries(&self.state);
                let stack_h = stacked_messages_height(&stack_entries, available_inner_w);
                let bottom_box_h = compute_bottom_box_height(stack_h, self.input_height, status_h);
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
                self.state.scroll.transcript_area = transcript_content_area;
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
                    &stack_entries,
                    stack_h,
                    self.input_height,
                    now_millis,
                );

                if has_side {
                    let side = horizontal[1];
                    let pane_style =
                        Style::default().fg(self.theme.row_assistant.fg.unwrap_or_default());
                    // The pane's foreground covers the whole rect, border included,
                    // so paint it before the chrome lays the rules down.
                    frame.render_widget(Block::default().style(pane_style), side);
                    let side_inner = Chrome::default()
                        .with_border(ContainerBorder::Plain)
                        .with_title("Events")
                        .draw(frame, side);
                    frame.render_widget(
                        Paragraph::new(Line::from("Events pane reserved")).style(pane_style),
                        side_inner,
                    );
                }

                match self.state.picker.render_kind() {
                    Some(PickerRenderKind::CommandPalette) => {
                        self.render_command_palette(frame, transcript_content_area);
                    }
                    Some(PickerRenderKind::Model) => {
                        self.render_model_picker(frame, transcript_content_area)
                    }
                    Some(PickerRenderKind::Agent) => {
                        self.render_agent_picker(frame, transcript_content_area)
                    }
                    Some(PickerRenderKind::Session) => {
                        self.render_session_picker(frame, transcript_content_area)
                    }
                    Some(PickerRenderKind::Theme) => {
                        self.render_theme_picker(frame, transcript_content_area)
                    }
                    Some(PickerRenderKind::InlineSlash) | None => {}
                }

                if self.state.info_panel.is_some() {
                    self.render_info_panel(frame, transcript_content_area);
                }
            })
            .map(|_completed| ())
            .map_err(|err| format!("TUI render failed: {err}"));
        let set_cursor_style = live
            .backend_mut()
            .set_cursor_style(cursor_style)
            .map_err(frame_sync_error);
        let end_sync = live.backend_mut().end_sync().map_err(frame_sync_error);

        begin_sync?;
        draw?;

        // Write back the resolved scroll offset so that scroll_offset always
        // reflects the actual rendered position. Without this, when following_tail is true
        // scroll_offset stays at 0 and the first scroll-up key jumps to the top.
        if let Some(offset) = rendered_scroll_offset {
            self.state.scroll.scroll_offset = offset;
        }

        set_cursor_style?;
        end_sync?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "../../test/runtime/coordinator.rs"]
mod coordinator_test;

/// Map a frame-control write failure to the render path's error string.
fn frame_sync_error(err: std::io::Error) -> String {
    format!("TUI frame sync failed: {err}")
}
