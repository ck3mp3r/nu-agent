use std::time::{Duration, Instant};

use crate::interaction::input::{TerminalEvent, TerminalKey};
use crate::runtime::test_driver::{CellProbe, DriveEvent, RenderLoopDriver};
use crate::runtime::{InputSourceDiagnostics, RuntimeCoordinator, TerminalEventSource};
use crate::state::{AppState, PickerRenderKind, TranscriptRole};
use agnt_core::transcript::ir::{Block, MessageRole, NoticeKind};
use agnt_core::transcript::items::Message;
use agnt_core::transcript::renderer::{ItemStatus, Renderable};

/// Push one Message block via the store's sole push API.
pub(super) fn push_message_line(
    state: &mut crate::state::AppState,
    role: MessageRole,
    text: impl Into<String>,
) {
    let msg = Message {
        role,
        markdown: text.into(),
    };
    state.transcript.push_block(Block {
        source: msg.source(),
        lane: msg.lane(),
        fill: msg.fill(),
        status: None,
    });
}

/// Map a block to the transcript role its source corresponds to.
pub(super) fn block_to_role(block: &agnt_core::transcript::ir::Block) -> TranscriptRole {
    use agnt_core::transcript::ir::BlockSource as Bs;
    match &block.source {
        Bs::Markdown {
            role: MessageRole::User,
            ..
        } => TranscriptRole::User,
        Bs::Markdown {
            role: MessageRole::Assistant,
            ..
        } => TranscriptRole::Assistant,
        Bs::Tool { .. } => TranscriptRole::Tool,
        Bs::ToolDisplay { .. } => TranscriptRole::ToolDisplay,
        Bs::Notice {
            kind: NoticeKind::Compaction,
            ..
        } => TranscriptRole::Compaction,
        Bs::Notice { .. } => TranscriptRole::System,
        Bs::Banner { .. } => TranscriptRole::System,
        Bs::Spacer => TranscriptRole::System,
    }
}

impl RuntimeCoordinator {
    pub(crate) fn new_for_test_with_watchdog(
        columns: u16,
        rows: u16,
        side_pane_visible: Option<bool>,
        input_watchdog_timeout: Duration,
    ) -> Self {
        Self::new_with_watchdog(columns, rows, side_pane_visible, input_watchdog_timeout)
    }

    pub(crate) fn state(&self) -> &AppState {
        &self.state
    }

    pub(crate) fn input_diagnostics_snapshot(&self) -> (String, String, Option<String>) {
        (
            self.input_backend_status.clone(),
            self.last_input_poll_status.clone(),
            self.last_input_error.clone(),
        )
    }

    pub(crate) fn render_needed(&self) -> bool {
        self.render_needed
    }

    pub(crate) fn set_render_needed(&mut self, needed: bool) {
        self.render_needed = needed;
    }

    pub(crate) fn set_last_render_at(&mut self, at: Instant) {
        self.last_render_at = at;
    }

    pub(crate) fn main_pane_rects_for_height(
        main_height: u16,
    ) -> (
        ratatui::layout::Rect,
        ratatui::layout::Rect,
        ratatui::layout::Rect,
        ratatui::layout::Rect,
    ) {
        crate::runtime::render::frame_test::main_pane_rects_for_height(main_height)
    }
}

pub(crate) fn modal_open_state_applies_dimmed_backdrop_for_test(state: &AppState) -> bool {
    state.picker.active().is_some() || state.info_panel.is_some()
}

pub(crate) fn inline_model_picker_modal_respects_border_and_backdrop_policy_for_test(
    state: &AppState,
) -> bool {
    state.picker.render_kind() == Some(PickerRenderKind::Model)
}

pub(crate) fn input_pane_content_width_for_test(inner_width: u16) -> usize {
    inner_width.saturating_sub(2) as usize
}

// region:    --- Test Support

/// The bar glyph the insert-mode caret paints as a buffer cell.
pub(super) const CARET_BAR: &str = "▏";

/// Row of the input's mode indicator, i.e. the input area's first row.
pub(super) fn indicator_row(driver: &RenderLoopDriver) -> super::Result<u16> {
    driver
        .buffer_text()
        .lines()
        .position(|line| line.contains('❯') || line.contains('❮'))
        .map(|row| row as u16)
        .ok_or_else(|| "should find the mode indicator row".into())
}

/// Column of the textarea's first cell on `row`: the first cell past the border
/// and the mode indicator.
pub(super) fn text_start_x(driver: &RenderLoopDriver, row: u16) -> super::Result<u16> {
    let buffer = driver.buffer_text();
    let line = buffer
        .lines()
        .nth(row as usize)
        .ok_or("should have the input row")?;
    let x = line
        .chars()
        .position(|c| !matches!(c, '│' | ' ' | '❯' | '❮'))
        .ok_or("should find the input text")?;
    Ok(x as u16)
}

/// The insert-mode caret cell, found by its bar glyph.
pub(super) fn caret_cell(driver: &RenderLoopDriver) -> super::Result<CellProbe> {
    driver
        .cell_with_symbol(CARET_BAR)
        .ok_or_else(|| "should paint the insert-mode caret bar cell".into())
}

/// Wraps a [`TerminalKey`] as a scripted driver event for the real render loop.
pub(super) fn key(key: TerminalKey) -> DriveEvent {
    DriveEvent::Key(TerminalEvent::Key(key))
}

pub(super) fn push_entry(coord: &mut RuntimeCoordinator, status: Option<ItemStatus>) {
    use agnt_core::transcript::items::Message;
    use agnt_core::transcript::renderer::Renderable;

    let msg = Message {
        role: MessageRole::Assistant,
        markdown: "hi".to_string(),
    };
    coord
        .state
        .transcript
        .push_block(agnt_core::transcript::ir::Block {
            source: msg.source(),
            lane: msg.lane(),
            fill: msg.fill(),
            status,
        });
}

// endregion: --- Test Support

// region:    --- Test Doubles

#[derive(Default)]
pub(super) struct ErrorEventSource;

impl TerminalEventSource for ErrorEventSource {
    fn poll_event(&mut self) -> core::result::Result<Option<TerminalEvent>, String> {
        Err("simulated source failure".to_string())
    }
}

#[derive(Clone)]
pub(super) struct ErrorWithDiagnosticsEventSource {
    pub(super) diagnostics: InputSourceDiagnostics,
    pub(super) error: String,
}

impl TerminalEventSource for ErrorWithDiagnosticsEventSource {
    fn poll_event(&mut self) -> core::result::Result<Option<TerminalEvent>, String> {
        Err(self.error.clone())
    }

    fn diagnostics(&self) -> InputSourceDiagnostics {
        self.diagnostics.clone()
    }
}

#[derive(Clone)]
pub(super) struct DiagnosticsOnlyEventSource {
    pub(super) diagnostics: InputSourceDiagnostics,
}

impl TerminalEventSource for DiagnosticsOnlyEventSource {
    fn poll_event(&mut self) -> core::result::Result<Option<TerminalEvent>, String> {
        Ok(None)
    }

    fn diagnostics(&self) -> InputSourceDiagnostics {
        self.diagnostics.clone()
    }
}

// endregion: --- Test Doubles
