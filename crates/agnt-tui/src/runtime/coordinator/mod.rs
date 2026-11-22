//! and render bookkeeping, and exposes the operations the TUI event loop and
//! renderer invoke.

use std::time::{Duration, Instant};

use crate::interaction::cancel::CancelController;
use crate::rendering::{
    layout::{INPUT_MIN_HEIGHT, MAIN_SIDE_MARGIN},
    theme::{ThemeName, TuiTheme},
};
use crate::runtime::frame_timing::{ArmTiming, FrameKind, FrameTiming, StreamArmTiming};
use crate::runtime::render::RowCache;
use crate::runtime::render::clock_cells::PaintedFrame;
use crate::runtime::status::RepoBranchTracker;
use crate::state::AppState;
use agnt_runtime::protocol::contracts::UiMessageSnapshot;

use clipboard::write_clipboard_text;

// region:    --- Modules

mod branch;
mod clipboard;
mod insert_mode;
mod instrument;
mod key_paths;
mod lifecycle;
mod paint;
mod projections;
mod reducers;
mod render;
mod terminal;

// endregion: --- Modules

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
    pub(super) fn replace_textarea(&mut self, lines: Vec<String>) {
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

    pub(super) fn sync_transcript_viewport_lines_with_layout(&mut self) {
        // With ListState, viewport lines are managed by ratatui automatically
        // No manual tracking needed
    }
}

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/runtime/coordinator.rs"]
mod coordinator_test;

// endregion: --- Tests
