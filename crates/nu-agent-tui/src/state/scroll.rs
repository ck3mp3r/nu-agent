//! Scroll/viewport/pane-focus domain state: transcript scroll offset,
//! tail-follow, cursor row, viewport metrics, rendered-line capture for yank,
//! transcript selection, entry visual info, and pane focus.

use super::selection::TranscriptSelection;
use super::{EntryVisualInfo, StatusState};
use crate::interaction::reducer::VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS;

const TRANSCRIPT_PAGE_LINES: usize = 8;

/// The vim-style pane focus of the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    Transcript,
    Input,
}

/// Scroll-domain state extracted from `AppState`.
///
/// Every scroll/viewport/pane-focus decision routes through this struct. The
/// transcript renderer reports its computed viewport metrics via
/// [`ScrollState::sync_after_render`] at the end of each frame.
#[derive(Debug, Clone)]
pub struct ScrollState {
    pub scroll_offset: usize,
    pub following_tail: bool,
    pub cursor_visual_row: usize,
    pub viewport_height: usize,
    pub max_scroll: usize,
    pub entry_indices: Vec<usize>,
    pub total_visual_rows: usize,
    pub rendered_line_text: Vec<String>,
    pub rendered_line_start_row: usize,
    pub selection: Option<TranscriptSelection>,
    pub entry_visual_info: Vec<EntryVisualInfo>,
    pub pane_focus: PaneFocus,
}

impl Default for ScrollState {
    fn default() -> Self {
        Self {
            scroll_offset: 0,
            following_tail: true,
            cursor_visual_row: 0,
            viewport_height: 0,
            max_scroll: 0,
            entry_indices: Vec::new(),
            total_visual_rows: 0,
            rendered_line_text: Vec::new(),
            rendered_line_start_row: 0,
            selection: None,
            entry_visual_info: Vec::new(),
            pane_focus: PaneFocus::Input,
        }
    }
}

/// Scroll-domain user actions, dispatched by the reducer.
/// `select` carries whether the input mode is Visual (selection extension).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAction {
    LineUp { select: bool },
    LineDown { select: bool },
    PageUp { select: bool },
    PageDown { select: bool },
    ToTop { select: bool },
    ToBottom { select: bool },
    FocusPaneLeft,
    FocusPaneRight,
}

impl ScrollState {
    // region:    --- Scroll action dispatch

    /// Reduce a scroll-domain action. Returns whether scroll state changed.
    /// The `select` flag is derived by the reducer from the input mode
    /// (orchestrator/input domain — not scroll state).
    pub fn reduce_scroll_action(&mut self, action: ScrollAction) -> bool {
        match action {
            ScrollAction::LineUp { select } => self.reduce_scroll_line_up(select),
            ScrollAction::LineDown { select } => self.reduce_scroll_line_down(select),
            ScrollAction::PageUp { select } => self.reduce_scroll_page_up(select),
            ScrollAction::PageDown { select } => self.reduce_scroll_page_down(select),
            ScrollAction::ToTop { select } => self.reduce_scroll_to_top(select),
            ScrollAction::ToBottom { select } => self.reduce_scroll_to_bottom(select),
            ScrollAction::FocusPaneLeft => self.reduce_focus_pane_left(),
            ScrollAction::FocusPaneRight => self.reduce_focus_pane_right(),
        }
    }

    fn reduce_scroll_line_up(&mut self, select: bool) -> bool {
        if self.following_tail {
            self.scroll_offset = self.max_scroll;
            self.following_tail = false;
        }
        self.cursor_visual_row = self.cursor_visual_row.saturating_sub(1);
        if select && let Some(sel) = &mut self.selection {
            sel.set_cursor(self.cursor_visual_row);
        }
        let scroll_margin = (self.viewport_height / 3).max(1);
        if self.cursor_visual_row < self.scroll_offset + scroll_margin {
            self.scroll_offset = self.scroll_offset.saturating_sub(1);
        }
        true
    }

    fn reduce_scroll_line_down(&mut self, select: bool) -> bool {
        if self.following_tail {
            self.scroll_offset = self.max_scroll;
            self.following_tail = false;
        }
        let max_visual_row = self.total_visual_rows.saturating_sub(1);
        self.cursor_visual_row = self.cursor_visual_row.saturating_add(1).min(max_visual_row);
        if select && let Some(sel) = &mut self.selection {
            sel.set_cursor(self.cursor_visual_row);
        }
        let scroll_margin = (self.viewport_height / 3).max(1);
        let viewport_bottom = self
            .scroll_offset
            .saturating_add(self.viewport_height)
            .saturating_sub(scroll_margin);
        if self.cursor_visual_row >= viewport_bottom {
            self.scroll_offset = self.scroll_offset.saturating_add(1);
        }
        true
    }

    fn reduce_scroll_page_up(&mut self, select: bool) -> bool {
        if self.following_tail {
            self.scroll_offset = self.max_scroll;
            self.following_tail = false;
        }
        let page = TRANSCRIPT_PAGE_LINES.max(1);
        self.cursor_visual_row = self.cursor_visual_row.saturating_sub(page);
        if select && let Some(sel) = &mut self.selection {
            sel.set_cursor(self.cursor_visual_row);
        }
        let scroll_margin = (self.viewport_height / 3).max(1);
        if self.cursor_visual_row < self.scroll_offset + scroll_margin {
            self.scroll_offset = self.scroll_offset.saturating_sub(page);
        }
        true
    }

    fn reduce_scroll_page_down(&mut self, select: bool) -> bool {
        if self.following_tail {
            self.scroll_offset = self.max_scroll;
            self.following_tail = false;
        }
        let page = TRANSCRIPT_PAGE_LINES.max(1);
        let max_visual_row = self.total_visual_rows.saturating_sub(1);
        self.cursor_visual_row = self
            .cursor_visual_row
            .saturating_add(page)
            .min(max_visual_row);
        if select && let Some(sel) = &mut self.selection {
            sel.set_cursor(self.cursor_visual_row);
        }
        let scroll_margin = (self.viewport_height / 3).max(1);
        let viewport_bottom = self
            .scroll_offset
            .saturating_add(self.viewport_height)
            .saturating_sub(scroll_margin);
        if self.cursor_visual_row >= viewport_bottom {
            self.scroll_offset = self.scroll_offset.saturating_add(page);
        }
        true
    }

    fn reduce_scroll_to_top(&mut self, select: bool) -> bool {
        if self.following_tail {
            self.scroll_offset = self.max_scroll;
            self.following_tail = false;
        }
        self.cursor_visual_row = 0;
        self.scroll_offset = 0;
        if select && let Some(sel) = &mut self.selection {
            sel.move_cursor_to_top();
        }
        true
    }

    fn reduce_scroll_to_bottom(&mut self, select: bool) -> bool {
        self.cursor_visual_row = self.total_visual_rows.saturating_sub(1);
        if select && let Some(sel) = &mut self.selection {
            sel.move_cursor_to_bottom(self.total_visual_rows.saturating_sub(1));
        }
        self.scroll_transcript_to_bottom();
        true
    }

    fn reduce_focus_pane_left(&mut self) -> bool {
        self.focus_prev_pane();
        true
    }

    fn reduce_focus_pane_right(&mut self) -> bool {
        self.focus_next_pane();
        true
    }

    // endregion: --- Scroll action dispatch

    // region:    --- Visual mode selection

    /// Attempts to start a visual-mode selection at the cursor row. Returns
    /// false when the transcript pane is not focused (the status line explains
    /// why); true when the selection started. The phase guard and the input
    /// mode transition stay with the reducer (orchestrator/input domains).
    pub fn enter_visual_mode(&mut self, status: &mut StatusState) -> bool {
        if self.pane_focus != PaneFocus::Transcript {
            status.status_line = VISUAL_REQUIRES_TRANSCRIPT_FOCUS_STATUS.to_string();
            return false;
        }
        self.selection = Some(TranscriptSelection::new(self.cursor_visual_row));
        status.status_line = "-- VISUAL --".to_string();
        true
    }

    /// Extracts the yank payload from the active selection and clears it.
    /// Returns None when the payload is empty. The clipboard request and the
    /// status line stay with the reducer (input/status domains).
    pub fn yank_selection(&mut self) -> Option<String> {
        let Some(sel) = &self.selection else {
            return None;
        };
        let (start_row, end_row) = sel.normalized_range();
        let offset = self.rendered_line_start_row;
        let payload: String = self
            .rendered_line_text
            .iter()
            .enumerate()
            .filter(|(i, _)| {
                let abs_row = offset + i;
                abs_row >= start_row && abs_row <= end_row
            })
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        self.selection = None;
        if payload.is_empty() {
            None
        } else {
            Some(payload)
        }
    }

    // endregion: --- Visual mode selection

    // region:    --- Render sync

    /// Applies the viewport metrics computed by the transcript renderer during
    /// the current frame: total visual rows, derived max scroll, and the
    /// tail-follow cursor pin (the cursor rides the last visual row while
    /// following the tail). Returns the clamped max scroll offset for the
    /// renderer's effective-offset computation.
    pub fn sync_after_render(&mut self, total_visual_rows: usize) -> usize {
        self.total_visual_rows = total_visual_rows;
        let max_scroll = total_visual_rows.saturating_sub(self.viewport_height);
        self.max_scroll = max_scroll;
        if self.following_tail {
            self.cursor_visual_row = total_visual_rows.saturating_sub(1);
        }
        max_scroll
    }

    // endregion: --- Render sync

    // region:    --- Scroll helpers

    pub fn scroll_transcript_line_up(&mut self) {
        self.following_tail = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn scroll_transcript_line_down(&mut self) {
        self.following_tail = false;
        self.scroll_offset = self.scroll_offset.saturating_add(1);
        // clamped to max_scroll at render time
    }

    pub fn scroll_transcript_page_up(&mut self, page_lines: usize) {
        self.following_tail = false;
        self.scroll_offset = self.scroll_offset.saturating_sub(page_lines.max(1));
    }

    pub fn scroll_transcript_page_down(&mut self, page_lines: usize) {
        self.following_tail = false;
        self.scroll_offset = self.scroll_offset.saturating_add(page_lines.max(1));
        // clamped to max_scroll at render time
    }

    pub fn scroll_transcript_to_top(&mut self) {
        self.following_tail = false;
        self.scroll_offset = 0;
    }

    pub fn scroll_transcript_to_bottom(&mut self) {
        self.following_tail = true;
    }

    pub fn focus_prev_pane(&mut self) {
        self.pane_focus = match self.pane_focus {
            PaneFocus::Transcript => PaneFocus::Input,
            PaneFocus::Input => PaneFocus::Transcript,
        };
    }

    pub fn focus_next_pane(&mut self) {
        self.pane_focus = match self.pane_focus {
            PaneFocus::Transcript => PaneFocus::Input,
            PaneFocus::Input => PaneFocus::Transcript,
        };
    }

    // endregion: --- Scroll helpers
}
