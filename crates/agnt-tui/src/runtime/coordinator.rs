//! The TUI runtime coordinator: owns the application state, input transport,
//! and render bookkeeping, and exposes the operations the TUI event loop and
//! renderer invoke.

use std::time::{Duration, Instant};

use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::layout::{Margin, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Paragraph};

use crate::interaction::input::{Key, UiInput};
use crate::interaction::reducer::RenderInput;
use crate::interaction::reducer::{
    narration_kind_label, narration_payload_bytes, record_kind_label, record_payload_bytes,
};
use crate::interaction::{
    cancel::CancelController,
    dispatch::{KeyLens, dispatch_terminal_event, rewrite_action},
    input::{TerminalEvent, TerminalKey},
};
use crate::rendering::{
    chrome::{Chrome, ContainerBorder},
    layout::{INPUT_MAX_HEIGHT, INPUT_MIN_HEIGHT, MAIN_SIDE_MARGIN, input_content_row_count},
    theme::{ThemeName, TuiTheme},
};
use crate::runtime::backend::FrameSync;
use crate::runtime::frame_timing::{ArmTiming, FrameKind, FrameTiming, StreamArmTiming};
use crate::runtime::layout::{STATS_BAND_ROWS, compute_bottom_box_height, compute_status_h};
use crate::runtime::render::RowCache;
use crate::runtime::render::bottom_box::{queued_prompt_entries, stacked_messages_height};
use crate::runtime::render::clock_cells::{ClockCell, PaintedFrame};
use crate::runtime::render::frame::current_time_millis;
use crate::runtime::render::transcript::PaintWindow;
use crate::runtime::status::{RepoBranchTracker, status_left_content, status_right_content};
use crate::state::{
    AppState, InputMode, McpServerState, McpServerUsabilityState, PickerRenderKind,
};
use agnt_render::transcript::renderer::ItemStatus;
use agnt_runtime::protocol::contracts::UiMessageSnapshot;
use agnt_runtime::protocol::events::{
    CompactionRecord, InferenceRecord, NarrationEvent, SessionRecord,
};
use agnt_runtime::protocol::skills::DiscoverableSkill as ProtocolDiscoverableSkill;
use agnt_runtime::tools::mcp::runtime::McpServerLifecycle;

/// The runtime coordinator that owns the TUI application state and the render
/// bookkeeping for the interactive render loop.
#[derive(Debug)]
pub struct RuntimeCoordinator {
    pub(crate) state: AppState,
    pub(crate) cancel_controller: CancelController,
    pub(crate) input_height: u16,
    last_input_content_width: usize,
    side_pane_visible: Option<bool>,
    pub(crate) quit_requested: bool,
    pub(crate) fatal_error: Option<String>,
    /// When a blocked quit or fatal delivery gives up, armed on the first failed
    /// attempt and measured on the tokio clock. `None` means no delivery is
    /// waiting.
    pub(crate) quit_delivery_deadline: Option<tokio::time::Instant>,
    pub(crate) repo_branch_tracker: Option<RepoBranchTracker>,
    pub(crate) theme_name: ThemeName,
    pub(crate) theme: TuiTheme,
    /// The frame the loop owes the surface, and what that frame repaints.
    /// `None` means nothing is pending. A clock tick raises a `ClockCells`
    /// frame only when no frame is pending; a state change always raises a
    /// `Recomposed` one and never downgrades.
    pub(crate) pending_frame: Option<FrameKind>,
    /// The clock reading the current frame paints at. The loop samples it when
    /// it paints, so every region of one frame reads the same time.
    pub(crate) frame_clock_millis: u128,
    /// The last painted frame: its bytes and the cells a clock step repaints.
    /// A clock-only frame restores these bytes instead of composing them.
    pub(crate) last_painted: Option<PaintedFrame>,
    pub(crate) last_render_at: Instant,
    /// Draft re-wraps the input path has paid for. An event that cannot write
    /// the draft must not move it.
    layout_recomputes: u64,
    /// The busy spinner's cadence step that last produced an animation frame.
    /// `None` means "not animating" — the next animated step renders at once.
    last_spinner_step: Option<u128>,
    /// What the loop's painting has cost so far. A frame's duration is the
    /// dominant latency source while the surface is busy, so the loop keeps the
    /// measurement where it paints.
    frame_timing: FrameTiming,
    /// What the loop's two fold arms have cost so far: the terminal arm folds
    /// keys, the stream arm folds bus records, and both block input while they
    /// run.
    arm_timing: ArmTiming,
    /// The stream arm's phase split and per-kind fold totals: the arm's single
    /// total cannot separate a slow fold from a descheduled loop.
    stream_arm_timing: StreamArmTiming,
    /// The rows each transcript entry painted last, so a busy-phase frame that
    /// changed nothing blits them instead of re-projecting every entry.
    pub(crate) row_cache: RowCache,
    pub(crate) textarea: ratatui_textarea::TextArea<'static>,
    /// The blocking pasteboard write, injected so a test can substitute it. A fn
    /// pointer, not a trait object: the workspace bans dynamic dispatch, and the
    /// seam needs nothing but the text.
    clipboard_writer: fn(&str) -> Result<(), String>,
    /// The copy the loop owes the pasteboard, running on the blocking pool. A
    /// pasteboard call is blocking OS IPC and must never run on the loop task.
    clipboard_copy: Option<tokio::task::JoinHandle<Result<(), String>>>,
}

impl RuntimeCoordinator {
    const MIN_FRAME_INTERVAL: Duration = Duration::from_millis(16);

    /// How long a blocked quit or fatal delivery keeps retrying before the loop
    /// gives up: the orchestrator is wedged, so exiting is the only recovery.
    const QUIT_DELIVERY_GIVE_UP: Duration = Duration::from_secs(2);

    pub fn new(columns: u16, _rows: u16, side_pane_visible: Option<bool>) -> Self {
        let theme_name = ThemeName::default();
        let theme = theme_name.resolve();
        let (event_tx, _event_rx) = tokio::sync::mpsc::channel::<
            agnt_runtime::orchestrator::events::OrchestratorEvent,
        >(256);
        let side_margin = if columns >= 8 { MAIN_SIDE_MARGIN } else { 0 };
        let last_input_content_width = columns.saturating_sub(2 * side_margin + 4) as usize;
        let mut coordinator = Self {
            state: AppState::new_with_sender(event_tx),
            cancel_controller: CancelController::default(),
            input_height: INPUT_MIN_HEIGHT,
            last_input_content_width,
            side_pane_visible,
            quit_requested: false,
            fatal_error: None,
            quit_delivery_deadline: None,
            repo_branch_tracker: None,
            theme_name,
            theme: theme.clone(),
            pending_frame: Some(FrameKind::Recomposed),
            frame_clock_millis: 0,
            last_painted: None,
            last_render_at: Instant::now() - Duration::from_millis(100),
            layout_recomputes: 0,
            last_spinner_step: None,
            frame_timing: FrameTiming::default(),
            arm_timing: ArmTiming::default(),
            stream_arm_timing: StreamArmTiming::default(),
            row_cache: RowCache::default(),
            textarea: Self::new_textarea(Vec::new()),
            clipboard_writer: write_clipboard_text,
            clipboard_copy: None,
        };
        coordinator.state.theme = theme;
        coordinator.sync_transcript_viewport_lines_with_layout();
        coordinator
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

    /// Consume one kernel-state snapshot from the facts channel.
    ///
    /// Returns whether the fold changed the rendered state.
    pub fn reduce_fact_record(&mut self, fact: agnt_runtime::bus::FactRecord) -> bool {
        self.state.status.reduce_fact_record(fact)
    }

    /// Consume one command from the control intake.
    ///
    /// The surface acts on the commands it owns and ignores the rest: a
    /// foreign command changes nothing.
    pub(crate) fn reduce_control(&mut self, command: &agnt_runtime::bus::ControlCommand) -> bool {
        let changed = self.reduce_render_input(RenderInput::Control(command));
        if changed {
            self.mark_render_needed();
        }
        changed
    }

    /// Consume one step of the turn spine.
    ///
    /// The surface acts on the steps it owns and ignores the rest: the fold's
    /// latch closes on the first live turn after a replay batch, and a foreign
    /// step changes nothing.
    pub(crate) fn reduce_lifecycle(&mut self, event: &agnt_runtime::bus::LifecycleEvent) -> bool {
        let changed = self.reduce_render_input(RenderInput::Lifecycle(event));
        if changed {
            self.mark_render_needed();
        }
        changed
    }

    /// Consume one surface input: a key, a wheel notch, or a resize.
    pub fn reduce_input(&mut self, input: UiInput) -> bool {
        let folded = crate::interaction::reducer::reduce(
            &mut self.state,
            input,
            Some(&self.cancel_controller),
        );
        // A fold that queued a shared UI action cannot apply it inline: the key
        // lens is still borrowing the state it dispatched from.
        folded | self.state.drain_launch_requests()
    }

    /// Consume one datum from a bus concern. This is the only reducer entry
    /// point for the bus: every record, narration event, lifecycle step, and
    /// control command converges here.
    pub(crate) fn reduce_render_input(&mut self, input: RenderInput<'_>) -> bool {
        let folded = crate::interaction::reducer::reduce(
            &mut self.state,
            input,
            Some(&self.cancel_controller),
        );
        folded | self.state.drain_launch_requests()
    }

    /// Drain the catalogs the pickers' reply hands delivered since the last
    /// poll. Returns whether a catalog changed the rendered state.
    pub(crate) fn poll_picker_replies(&mut self) -> bool {
        let changed = self.state.poll_picker_replies();
        if changed {
            self.mark_render_needed();
        }
        changed
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
    ///
    /// The read runs on the blocking pool: git ref reads are blocking file I/O
    /// and never run on the loop task.
    pub(crate) async fn refresh_repo_branch(&mut self) {
        if let Some(tracker) = self.repo_branch_tracker.as_mut() {
            tracker.refresh_off_loop().await;
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

    /// Track a quit or fatal delivery the orchestrator has not taken yet.
    ///
    /// The deadline is armed on the first blocked attempt and measured on the
    /// tokio clock, so a paused-time test advances it deterministically. Past
    /// the deadline the consumer is wedged, and exiting is the only recovery.
    pub(crate) fn quit_delivery_expired(&mut self) -> bool {
        let deadline = *self
            .quit_delivery_deadline
            .get_or_insert_with(|| tokio::time::Instant::now() + Self::QUIT_DELIVERY_GIVE_UP);
        tokio::time::Instant::now() >= deadline
    }

    /// Forget a delivery deadline: the event landed, so the retry is over.
    pub(crate) fn clear_quit_delivery_deadline(&mut self) {
        self.quit_delivery_deadline = None;
    }

    pub fn event_sender(
        &self,
    ) -> &tokio::sync::mpsc::Sender<agnt_runtime::orchestrator::events::OrchestratorEvent> {
        &self.state.event_tx
    }

    pub fn take_cancel_requested(&self) -> bool {
        self.cancel_controller.take_cancel_requested()
    }

    /// Process a single terminal event received from the input source: route
    /// insert-mode characters/paste into the textarea, and dispatch everything
    /// else (submit, quit, navigation, pickers) through the reducer. Called by
    /// the async render loop for every terminal event it receives.
    pub fn handle_terminal_event(&mut self, event: TerminalEvent) {
        // Pick up any restored input text from cancelled prompts before
        // processing the next event. A key, a paste, and a resize re-wrap the
        // draft in their own paths below, so the pickup leaves the re-wrap to
        // them; every other event carries the pickup alone, and the replaced
        // textarea's height must be computed here or it keeps the previous
        // draft's size.
        self.pickup_restored_input_text(!matches!(
            event,
            TerminalEvent::Key(_) | TerminalEvent::Paste(_) | TerminalEvent::Resize(_)
        ));

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
        // Only a key or a paste can write the draft from here: a resize
        // re-wrapped it at the new content width above, and a wheel notch
        // scrolls the transcript without touching the draft. Re-wrapping an
        // untouched draft costs O(draft) per notch, so a long draft would make
        // every scroll step pay for text it cannot have changed.
        if matches!(event, TerminalEvent::Key(_) | TerminalEvent::Paste(_)) {
            self.recompute_layout_for_current_input();
        }
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
    ///
    /// `recompute_layout` says whether the event that carries the pickup re-wraps
    /// the draft itself: a key, a paste, and a resize do, a wheel notch does not.
    /// The replaced textarea inherits the previous draft's wrapped height, so the
    /// event that writes it must re-wrap it — once, from whichever side owns it.
    fn pickup_restored_input_text(&mut self, recompute_layout: bool) {
        if let Some(text) = self.state.input.restored_input_text.take() {
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            let last_line = lines.len().saturating_sub(1) as u16;
            let last_col = lines.last().map(|l| l.len()).unwrap_or(0) as u16;
            self.replace_textarea(lines);
            self.textarea
                .move_cursor(ratatui_textarea::CursorMove::Jump(last_line, last_col));
            if recompute_layout {
                self.recompute_layout_for_current_input();
            }
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
                        let _ = self.reduce_input(UiInput::KeyPressed { key: Key::Char(ch) });
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
                    let _ = self.reduce_input(UiInput::KeyPressed { key: Key::Char(ch) });
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
                let changed = self.reduce_input(UiInput::KeyPressed { key: submit_key });
                self.replace_textarea(Vec::new());
                self.state.check_inline_slash("");
                self.mark_render_needed();
                changed
            }
            TerminalKey::Esc => {
                // A running turn reroutes the press to the reducer. The slash
                // list closes there and the press ends at the surface: a
                // surface-close press never arms the abort.
                if self.state.phase != crate::state::UiPhase::Idle {
                    return false;
                }
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
        self.layout_recomputes = self.layout_recomputes.saturating_add(1);
        self.input_height =
            input_content_row_count(&self.textarea, self.last_input_content_width as u16)
                .clamp(INPUT_MIN_HEIGHT, INPUT_MAX_HEIGHT);
    }

    /// Hand a pending yank to the blocking pool.
    ///
    /// The pasteboard call is blocking OS IPC, so it never runs on the loop
    /// task. One copy is in flight at a time: a request set while one runs stays
    /// pending and starts on the first flush after
    /// [`Self::poll_clipboard_outcome`] folds the outcome.
    fn flush_clipboard_request(&mut self) {
        if self.clipboard_copy.is_some() {
            return;
        }
        let Some(text) = self.state.input.take_clipboard_request() else {
            return;
        };
        let writer = self.clipboard_writer;
        self.clipboard_copy = Some(tokio::task::spawn_blocking(move || writer(&text)));
    }

    /// Fold a finished copy, if there is one. Returns whether the fold changed
    /// the rendered state.
    ///
    /// Only a finished handle is awaited, so this never waits: a copy still
    /// running stays pending for a later poll.
    pub(crate) async fn poll_clipboard_outcome(&mut self) -> bool {
        let Some(copy) = self.clipboard_copy.take() else {
            return false;
        };
        if !copy.is_finished() {
            self.clipboard_copy = Some(copy);
            return false;
        }
        let failure = match copy.await {
            Ok(Ok(())) => return false,
            Ok(Err(error)) => error,
            Err(join_error) => join_error.to_string(),
        };
        self.state
            .status
            .message
            .set_message(format!("Clipboard copy failed: {failure}"));
        true
    }

    /// Fold a batch of stream records under the chunk-coalescing rule: at most
    /// the latest inference chunk and the latest compaction summary chunk are
    /// held; a pending chunk flushes before any other record so the streamed
    /// text keeps its order; a newer chunk of the same kind replaces the older
    /// one. Returns whether any fold changed state.
    pub(crate) fn reduce_record_batch(
        &mut self,
        records: impl IntoIterator<Item = SessionRecord>,
    ) -> bool {
        self.fold_batch(
            records,
            |record| {
                matches!(
                    record,
                    SessionRecord::Inference(InferenceRecord::Chunk { .. })
                )
            },
            |record| {
                matches!(
                    record,
                    SessionRecord::Compaction(CompactionRecord::SummaryChunk { .. })
                )
            },
            record_kind_label,
            record_payload_bytes,
            |coordinator, record| coordinator.reduce_render_input(RenderInput::Record(record)),
        )
    }

    /// Fold a batch of narration events under the same chunk-coalescing rule.
    pub(crate) fn reduce_narration_batch(
        &mut self,
        events: impl IntoIterator<Item = NarrationEvent>,
    ) -> bool {
        self.fold_batch(
            events,
            |event| matches!(event, NarrationEvent::InferenceChunk { .. }),
            |event| matches!(event, NarrationEvent::CompactionSummaryChunk { .. }),
            narration_kind_label,
            narration_payload_bytes,
            |coordinator, event| coordinator.reduce_render_input(RenderInput::Narration(event)),
        )
    }

    /// The coalescing skeleton both batches share: hold the newest chunk of each
    /// kind, flush the held ones before any other item, and flush the tail.
    fn fold_batch<T>(
        &mut self,
        items: impl IntoIterator<Item = T>,
        is_inference_chunk: impl Fn(&T) -> bool,
        is_compaction_chunk: impl Fn(&T) -> bool,
        kind_label: impl Fn(&T) -> &'static str,
        payload_bytes: impl Fn(&T) -> u128,
        mut fold: impl FnMut(&mut Self, &T) -> bool,
    ) -> bool {
        let mut pending_inference: Option<T> = None;
        let mut pending_compaction: Option<T> = None;
        let mut changed = false;

        for item in items {
            if is_inference_chunk(&item) {
                pending_inference = Some(item);
                continue;
            }
            if is_compaction_chunk(&item) {
                pending_compaction = Some(item);
                continue;
            }
            if let Some(pending) = pending_inference.take() {
                changed |= self.fold_one(&pending, &mut fold, &kind_label, &payload_bytes);
            }
            if let Some(pending) = pending_compaction.take() {
                changed |= self.fold_one(&pending, &mut fold, &kind_label, &payload_bytes);
            }
            changed |= self.fold_one(&item, &mut fold, &kind_label, &payload_bytes);
        }

        // Flush the tail of the batch.
        if let Some(pending) = pending_inference.take() {
            changed |= self.fold_one(&pending, &mut fold, &kind_label, &payload_bytes);
        }
        if let Some(pending) = pending_compaction.take() {
            changed |= self.fold_one(&pending, &mut fold, &kind_label, &payload_bytes);
        }

        if changed {
            self.mark_render_needed();
        }
        changed
    }

    /// Fold one item under the arm's per-kind instrumentation: the label names
    /// the record kind, and the elapsed time joins that kind's running total.
    ///
    /// The measure is the fold call alone; nothing here reads the payload, so the
    /// instrument adds no O(payload) scan.
    fn fold_one<T>(
        &mut self,
        item: &T,
        fold: &mut impl FnMut(&mut Self, &T) -> bool,
        kind_label: &impl Fn(&T) -> &'static str,
        payload_bytes: &impl Fn(&T) -> u128,
    ) -> bool {
        let started = Instant::now();
        let changed = fold(self, item);
        let elapsed_ms = started.elapsed().as_millis();
        self.stream_arm_timing
            .record_fold(kind_label(item), elapsed_ms, payload_bytes(item));
        changed
    }

    /// Raise a full frame: the state changed, so every region is composed
    /// again. A pending clock-only frame never survives a state change.
    pub(crate) fn mark_render_needed(&mut self) {
        self.pending_frame = Some(FrameKind::Recomposed);
    }

    /// Raise a clock-only frame: the spinner step advanced and nothing else
    /// changed, so only the cells that read the frame clock are repainted. A
    /// frame already pending stays as it is — a recomposition covers the clock
    /// cells too, and a state change never downgrades to a repaint.
    ///
    /// The render-stats row carries the loop's own per-frame counters, so a
    /// visible row changes with every frame: it must be composed again, which
    /// makes the tick a full frame.
    pub(crate) fn mark_clock_tick(&mut self) {
        if self.state.render_stats_visible {
            self.mark_render_needed();
            return;
        }
        self.pending_frame = self.pending_frame.or(Some(FrameKind::ClockCells));
    }

    /// Draft re-wraps the input path has paid for since construction.
    pub(crate) fn layout_recomputes(&self) -> u64 {
        self.layout_recomputes
    }

    pub(crate) fn has_active_animation(&self) -> bool {
        crate::runtime::status::model_activity_label(&self.state) == "busy"
            || self.state.transcript.entries().iter().any(|entry| {
                matches!(
                    entry,
                    agnt_render::transcript::entry::TranscriptEntry::Tool(tool)
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
        let abort_expired = self.state.sweep_expired_abort(now);
        if self.state.status.message.expired(now) {
            self.state.status.message.clear();
            return true;
        }
        abort_expired
    }

    pub(crate) fn has_pending_status_message(&self) -> bool {
        self.state.status.message.is_pending()
    }

    /// The loop's painting cost so far: painted frames, the last frame's
    /// duration, and the count of frames that crossed one spinner cadence.
    pub(crate) fn frame_timing(&self) -> &FrameTiming {
        &self.frame_timing
    }

    /// Frames painted within the last second of the frame clock: the reading the
    /// HUD shows. The cumulative counters stay for the log side and `slow`.
    pub(crate) fn fps(&mut self) -> u64 {
        self.frame_timing.fps(self.frame_clock_millis)
    }

    /// The loop's fold-arm durations: the terminal arm's and the stream arm's
    /// last elapsed value, each classified against the warn bound its caller
    /// passes.
    pub(crate) fn arm_timing(&self) -> &ArmTiming {
        &self.arm_timing
    }

    /// Record one completed input-arm fold. The loop passes the elapsed value
    /// its warn check reads and the same bound, so the recorded duration and
    /// the warn line can never disagree about which arm was slow.
    pub(crate) fn record_input_arm(&mut self, elapsed_ms: u128, slow_warn_ms: u128) -> bool {
        self.arm_timing.record_input_arm(elapsed_ms, slow_warn_ms)
    }

    /// Record one completed stream arm, on the same terms as the input arm.
    pub(crate) fn record_stream_arm(&mut self, elapsed_ms: u128, slow_warn_ms: u128) -> bool {
        self.arm_timing.record_stream_arm(elapsed_ms, slow_warn_ms)
    }

    /// Record how long the last input arm's first event waited for its fold.
    ///
    /// The loop passes the wait it measured and the bound its warn check reads,
    /// so the recorded value and the warn line can never disagree.
    pub(crate) fn record_input_wait(&mut self, wait_ms: u128, warn_ms: u128) -> bool {
        self.arm_timing.record_input_wait(wait_ms, warn_ms)
    }

    /// Record how far past its deadline the last render tick woke, on the same
    /// terms as the input wait.
    pub(crate) fn record_tick_late(&mut self, late_ms: u128, warn_ms: u128) -> bool {
        self.arm_timing.record_tick_late(late_ms, warn_ms)
    }

    /// Begin a stream arm: reset the phase split and snapshot the store's
    /// running totals, so the arm reports only what it accrued.
    pub(crate) fn begin_stream_arm(&mut self) {
        self.stream_arm_timing.reset(
            self.state.transcript.remeasure_cost(),
            self.state.transcript.diff_gate_cost(),
        );
    }

    /// The arm's phase split, for the phases the loop times itself.
    pub(crate) fn stream_arm_timing_mut(&mut self) -> &mut StreamArmTiming {
        &mut self.stream_arm_timing
    }

    /// Close the arm's store deltas and render its one-line breakdown.
    pub(crate) fn stream_arm_breakdown(&mut self, arm_ms: u128) -> String {
        self.stream_arm_timing.close_store(
            self.state.transcript.remeasure_cost(),
            self.state.transcript.diff_gate_cost(),
            self.state.transcript.last_remeasure_index(),
        );
        self.stream_arm_timing.format_breakdown(arm_ms)
    }

    pub(crate) fn render_if_needed<B: ratatui::backend::Backend + FrameSync>(
        &mut self,
        live: &mut Option<&mut ratatui::Terminal<B>>,
    ) -> Result<(), String> {
        let Some(kind) = self.pending_frame else {
            return Ok(());
        };
        if self.last_render_at.elapsed() < Self::MIN_FRAME_INTERVAL {
            return Ok(());
        }
        self.pending_frame = None;
        self.last_render_at = Instant::now();
        self.theme = self.state.theme.clone();
        // One reading per frame: every region of the frame names the same time.
        self.frame_clock_millis = current_time_millis();
        match kind {
            FrameKind::Recomposed => self.render_frame(live),
            FrameKind::ClockCells => self.render_clock_frame(live),
        }
    }

    /// Compose the whole frame from the state, at the stored frame clock.
    pub(crate) fn render_frame<B: ratatui::backend::Backend + FrameSync>(
        &mut self,
        live: &mut Option<&mut ratatui::Terminal<B>>,
    ) -> Result<(), String> {
        self.paint(live, FrameKind::Recomposed)
    }

    /// Repaint the cells that read the frame clock over the last painted frame,
    /// at the stored frame clock.
    pub(crate) fn render_clock_frame<B: ratatui::backend::Backend + FrameSync>(
        &mut self,
        live: &mut Option<&mut ratatui::Terminal<B>>,
    ) -> Result<(), String> {
        self.paint(live, FrameKind::ClockCells)
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

    /// Paint the frame `kind` asks for, inside one synchronized-output block.
    ///
    /// Both kinds share the block's prologue and epilogue: the cursor shape and
    /// the block itself are frame-level writes, so they belong to the frame
    /// whichever way its body is painted.
    fn paint<B: ratatui::backend::Backend + FrameSync>(
        &mut self,
        live: &mut Option<&mut ratatui::Terminal<B>>,
        kind: FrameKind,
    ) -> Result<(), String> {
        let Some(live) = live.as_mut() else {
            return Ok(());
        };
        let frame_started = Instant::now();
        let cursor_style = self.state.input.mode.cursor_style();

        // A synchronized-output block must never outlive its frame: the terminal
        // would keep buffering and stop repainting. So open the block, run every
        // phase, and only then report the first failure.
        let begin_sync = live.backend_mut().begin_sync().map_err(frame_sync_error);

        // Values the FnOnce draw closure smuggles back out: the bytes the frame
        // left on the surface, the resolved scroll offset, the cells a clock
        // step repaints, and whether this frame was the clock-only repaint.
        let mut painted_buffer: Option<ratatui::buffer::Buffer> = None;
        let mut rendered_scroll_offset: Option<usize> = None;
        let mut clock_cells: Vec<ClockCell> = Vec::new();
        let mut clock_only = false;

        let draw = live
            .draw(|frame| {
                clock_only = match kind {
                    FrameKind::ClockCells => self.repaint_clock_cells(frame),
                    FrameKind::Recomposed => false,
                };
                if !clock_only {
                    self.paint_recomposed(frame, &mut rendered_scroll_offset, &mut clock_cells);
                }
            })
            .map(|completed| painted_buffer = Some(completed.buffer.clone()))
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

        // The snapshot a clock-only frame patches. Its clock cells stay when the
        // frame was one: the next step reads the same cells.
        if let Some(buffer) = painted_buffer {
            match self.last_painted.as_mut() {
                Some(painted) if clock_only => painted.replace_buffer(buffer),
                _ => self.last_painted = Some(PaintedFrame::new(buffer, clock_cells)),
            }
        }

        set_cursor_style?;
        end_sync?;
        let painted_kind = if clock_only {
            FrameKind::ClockCells
        } else {
            FrameKind::Recomposed
        };
        let frame_elapsed = frame_started.elapsed();
        if self
            .frame_timing
            .record_frame(frame_elapsed, painted_kind, self.frame_clock_millis)
        {
            let timing = self.frame_timing();
            log::warn!(
                "tui-frame-slow: frame paint took {} ms; {} of {} painted frames slow ({} recomposed, {} clock-only); {} input re-wraps; row cache: {} hits, {} misses, {} parked entries ({} rows), {} evicted, {} invalidations",
                timing.last_frame_millis(),
                timing.slow_frames(),
                timing.frames_painted(),
                timing.recomposed_frames(),
                timing.clock_frames(),
                self.layout_recomputes(),
                self.row_cache.hits(),
                self.row_cache.misses(),
                self.row_cache.parked_len(),
                self.row_cache.parked_rows(),
                self.row_cache.evicted_total(),
                self.row_cache.invalidations()
            );
        }
        Ok(())
    }

    /// Compose every region of the frame from the state, and collect the cells a
    /// clock step repaints.
    fn paint_recomposed(
        &mut self,
        frame: &mut ratatui::Frame,
        rendered_scroll_offset: &mut Option<usize>,
        clock_cells: &mut Vec<ClockCell>,
    ) {
        // Capture values needed inside the body (avoid borrowing self mutably)
        let transcript_following_tail = self.state.scroll.following_tail;
        let transcript_scroll_offset = self.state.scroll.scroll_offset;

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
            let probe = status_left_content(None, &self.state, &self.theme, available_inner_w);
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
        let bottom_box_h = compute_bottom_box_height(
            stack_h,
            self.input_height,
            status_h,
            u16::from(self.state.render_stats_visible).saturating_mul(STATS_BAND_ROWS),
        );
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
            clock_cells.extend(self.render_transcript_pane(
                frame,
                transcript_content_area,
                transcript_list_area,
                transcript_following_tail,
                transcript_scroll_offset,
                rendered_scroll_offset,
            ));
        }

        let now_millis = self.frame_clock_millis;
        clock_cells.extend(self.render_bottom_box(
            frame,
            vertical[2],
            &stack_entries,
            stack_h,
            self.input_height,
            now_millis,
        ));

        if has_side {
            let side = horizontal[1];
            let pane_style = Style::default().fg(self.theme.row_assistant.fg.unwrap_or_default());
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
    }
}

#[cfg(test)]
#[path = "../../test/runtime/coordinator.rs"]
mod coordinator_test;

/// Map a frame-control write failure to the render path's error string.
fn frame_sync_error(err: std::io::Error) -> String {
    format!("TUI frame sync failed: {err}")
}

/// The production clipboard writer: one blocking pasteboard call.
///
/// Only ever reached through [`RuntimeCoordinator::flush_clipboard_request`],
/// which runs it on the blocking pool — never on the loop task.
fn write_clipboard_text(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text))
        .map_err(|error| error.to_string())
}
