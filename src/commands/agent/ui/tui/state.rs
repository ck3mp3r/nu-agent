use crate::commands::agent::ui::tui::markdown::rendered_line_to_plain_text;
use crate::commands::agent::ui::tui::{selection::TranscriptSelection, viewport::TranscriptViewport};
use ratatui::text::Line;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPhase {
    Idle,
    Busy,
    AbortPending,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Insert,
    Normal,
    Visual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneFocus {
    Transcript,
    Input,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptRole {
    User,
    Assistant,
    System,
    Tool,
    Separator,
}

const TURN_SEPARATOR_LINE: &str = "────────────────";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptLine {
    pub role: TranscriptRole,
    pub text: String,
    pub rendered: Option<Line<'static>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct InputState {
    pub buffer: String,
    pub locked: bool,
    pub cursor: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AbortState {
    pub pending: bool,
    pub confirmation_marker: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppState {
    pub phase: UiPhase,
    pub input: InputState,
    pub abort: AbortState,
    pub transcript_preview: Vec<TranscriptLine>,
    pub transcript_follow_tail: bool,
    pub transcript_scroll_lines_from_bottom: usize,
    pub transcript_viewport_lines: usize,
    pub status_line: String,
    pub input_mode: InputMode,
    pub pane_focus: PaneFocus,
    pub latest_input_tokens: Option<u64>,
    pub latest_output_tokens: Option<u64>,
    pub latest_total_tokens: Option<u64>,
    pub session_total_tokens: u64,
    pub quit_requested: bool,
    active_cycle: bool,
    insert_exit_pending_j: bool,
    normal_pending_key: Option<char>,
    transcript_cursor: Option<usize>,
    visual_selection: Option<TranscriptSelection>,
    clipboard_request: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            phase: UiPhase::Idle,
            input: InputState::default(),
            abort: AbortState::default(),
            transcript_preview: Vec::new(),
            transcript_follow_tail: true,
            transcript_scroll_lines_from_bottom: 0,
            transcript_viewport_lines: 1,
            status_line: String::new(),
            input_mode: InputMode::Insert,
            pane_focus: PaneFocus::Input,
            latest_input_tokens: None,
            latest_output_tokens: None,
            latest_total_tokens: None,
            session_total_tokens: 0,
            quit_requested: false,
            active_cycle: false,
            insert_exit_pending_j: false,
            normal_pending_key: None,
            transcript_cursor: None,
            visual_selection: None,
            clipboard_request: None,
        }
    }
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_active_cycle(&self) -> bool {
        self.active_cycle
    }

    pub fn append_input_char(&mut self, ch: char) {
        if self.input.locked {
            self.ensure_invariants();
            return;
        }

        if self.input.cursor >= self.input.buffer.len() {
            self.input.buffer.push(ch);
            self.input.cursor = self.input.buffer.len();
        } else {
            self.input.buffer.insert(self.input.cursor, ch);
            self.input.cursor += ch.len_utf8();
        }

        self.ensure_invariants();
    }

    pub fn enter_insert_mode(&mut self) {
        self.input_mode = InputMode::Insert;
        self.insert_exit_pending_j = false;
        self.normal_pending_key = None;
        self.pane_focus = PaneFocus::Input;
        self.visual_selection = None;
    }

    pub fn enter_normal_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.insert_exit_pending_j = false;
        self.normal_pending_key = None;
        self.pane_focus = PaneFocus::Transcript;
        self.visual_selection = None;
    }

    pub fn enter_visual_mode(&mut self) {
        self.input_mode = InputMode::Visual;
        self.insert_exit_pending_j = false;
        self.normal_pending_key = None;
        self.pane_focus = PaneFocus::Transcript;
        self.visual_selection = self
            .current_transcript_cursor_index()
            .map(TranscriptSelection::new);
    }

    pub fn set_insert_exit_pending_j(&mut self, pending: bool) {
        self.insert_exit_pending_j = pending;
    }

    pub fn insert_exit_pending_j(&self) -> bool {
        self.insert_exit_pending_j
    }

    pub fn clear_normal_pending_key(&mut self) {
        self.normal_pending_key = None;
    }

    pub fn arm_normal_pending_key(&mut self, key: char) {
        self.normal_pending_key = Some(key);
    }

    pub fn take_normal_pending_key_if(&mut self, key: char) -> bool {
        let matches = self.normal_pending_key == Some(key);
        self.normal_pending_key = None;
        matches
    }

    pub fn selected_transcript_range(&self) -> Option<(usize, usize)> {
        if self.input_mode != InputMode::Visual {
            return None;
        }
        self.visual_selection
            .as_ref()
            .and_then(|selection| selection.bounded_range(self.transcript_preview.len()))
    }

    pub fn visual_anchor_index(&self) -> Option<usize> {
        self.visual_selection.as_ref().map(TranscriptSelection::anchor)
    }

    pub fn visual_cursor_index(&self) -> Option<usize> {
        self.visual_selection.as_ref().map(TranscriptSelection::cursor)
    }

    pub fn transcript_cursor_index(&self) -> Option<usize> {
        self.transcript_cursor
    }

    pub fn set_transcript_viewport_lines(&mut self, lines: usize) {
        let mut model = self.transcript_viewport_model();
        model.set_viewport_lines(lines.max(1));
        self.apply_transcript_viewport_model(&model);
    }

    pub fn extend_visual_cursor_line_up(&mut self) {
        if self.input_mode != InputMode::Visual {
            return;
        }
        if self.visual_selection.is_none() {
            return;
        }
        self.transcript_follow_tail = false;
        let mut model = self.transcript_viewport_model();
        model.line_up();
        if let (Some(selection), Some(cursor)) =
            (self.visual_selection.as_mut(), model.current_cursor_index())
        {
            selection.set_cursor(cursor);
        }
        self.apply_transcript_viewport_model(&model);
    }

    pub fn extend_visual_cursor_line_down(&mut self) {
        if self.input_mode != InputMode::Visual {
            return;
        }
        if self.visual_selection.is_none() {
            return;
        }
        let mut model = self.transcript_viewport_model();
        model.line_down();
        if let (Some(selection), Some(cursor)) =
            (self.visual_selection.as_mut(), model.current_cursor_index())
        {
            selection.set_cursor(cursor);
        }
        self.apply_transcript_viewport_model(&model);
    }

    pub fn extend_visual_cursor_page_up(&mut self, page_lines: usize) {
        if self.input_mode != InputMode::Visual {
            return;
        }
        if self.visual_selection.is_none() {
            return;
        }
        self.transcript_follow_tail = false;
        let mut model = self.transcript_viewport_model();
        model.page_up(page_lines.max(1));
        if let (Some(selection), Some(cursor)) =
            (self.visual_selection.as_mut(), model.current_cursor_index())
        {
            selection.set_cursor(cursor);
        }
        self.apply_transcript_viewport_model(&model);
    }

    pub fn extend_visual_cursor_page_down(&mut self, page_lines: usize) {
        if self.input_mode != InputMode::Visual {
            return;
        }
        if self.visual_selection.is_none() {
            return;
        }
        let mut model = self.transcript_viewport_model();
        model.page_down(page_lines.max(1));
        if let (Some(selection), Some(cursor)) =
            (self.visual_selection.as_mut(), model.current_cursor_index())
        {
            selection.set_cursor(cursor);
        }
        self.apply_transcript_viewport_model(&model);
    }

    pub fn extend_visual_cursor_to_top(&mut self) {
        if self.input_mode != InputMode::Visual {
            return;
        }
        if self.visual_selection.is_none() {
            return;
        }
        let mut model = self.transcript_viewport_model();
        model.jump_top();
        if let (Some(selection), Some(cursor)) =
            (self.visual_selection.as_mut(), model.current_cursor_index())
        {
            selection.set_cursor(cursor);
        }
        self.apply_transcript_viewport_model(&model);
    }

    pub fn extend_visual_cursor_to_bottom(&mut self) {
        if self.input_mode != InputMode::Visual {
            return;
        }
        if self.visual_selection.is_none() {
            return;
        }
        let mut model = self.transcript_viewport_model();
        model.jump_bottom();
        if let (Some(selection), Some(cursor)) =
            (self.visual_selection.as_mut(), model.current_cursor_index())
        {
            selection.set_cursor(cursor);
        }
        self.apply_transcript_viewport_model(&model);
    }

    pub fn queue_visual_selection_to_clipboard(&mut self) {
        let Some((start, end)) = self.selected_transcript_range() else {
            return;
        };
        let text = self.transcript_preview[start..=end]
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        self.clipboard_request = Some(text);
    }

    pub fn take_clipboard_request(&mut self) -> Option<String> {
        self.clipboard_request.take()
    }

    pub fn backspace_input_char(&mut self) {
        if self.input.locked {
            self.ensure_invariants();
            return;
        }

        if let Some(start) = previous_char_start(&self.input.buffer, self.input.cursor) {
            self.input.buffer.drain(start..self.input.cursor);
            self.input.cursor = start;
        }

        self.ensure_invariants();
    }

    pub fn delete_input_char(&mut self) {
        if self.input.locked {
            self.ensure_invariants();
            return;
        }

        if let Some(end) = next_char_end(&self.input.buffer, self.input.cursor) {
            self.input.buffer.drain(self.input.cursor..end);
        }

        self.ensure_invariants();
    }

    pub fn move_cursor_left(&mut self) {
        if self.input.locked {
            self.ensure_invariants();
            return;
        }

        if let Some(start) = previous_char_start(&self.input.buffer, self.input.cursor) {
            self.input.cursor = start;
        }

        self.ensure_invariants();
    }

    pub fn move_cursor_right(&mut self) {
        if self.input.locked {
            self.ensure_invariants();
            return;
        }

        if let Some(end) = next_char_end(&self.input.buffer, self.input.cursor) {
            self.input.cursor = end;
        }

        self.ensure_invariants();
    }

    pub fn move_cursor_home(&mut self) {
        if !self.input.locked {
            self.input.cursor = 0;
        }
        self.ensure_invariants();
    }

    pub fn move_cursor_end(&mut self) {
        if !self.input.locked {
            self.input.cursor = self.input.buffer.len();
        }
        self.ensure_invariants();
    }

    pub fn accept_submit(&mut self) {
        self.input.buffer.clear();
        self.input.cursor = 0;
        self.phase = UiPhase::Busy;
        self.active_cycle = true;
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn request_abort_confirmation(&mut self) -> bool {
        if self.phase != UiPhase::Busy || !self.active_cycle {
            self.ensure_invariants();
            return false;
        }

        self.abort.pending = true;
        self.abort.confirmation_marker = self.abort.confirmation_marker.saturating_add(1);
        self.phase = UiPhase::AbortPending;
        self.ensure_invariants();
        true
    }

    pub fn finalize_cycle(&mut self) {
        self.active_cycle = false;
        self.phase = UiPhase::Idle;
        self.abort.pending = false;
        self.ensure_invariants();
    }

    pub fn push_transcript_line(&mut self, role: TranscriptRole, line: impl Into<String>) {
        self.push_transcript_entry(role, line.into(), None);
    }

    pub fn push_transcript_rendered_line(&mut self, role: TranscriptRole, line: Line<'static>) {
        let text = rendered_line_to_plain_text(&line);
        self.push_transcript_entry(role, text, Some(line));
    }

    fn push_transcript_entry(
        &mut self,
        role: TranscriptRole,
        text: String,
        rendered: Option<Line<'static>>,
    ) {
        if should_insert_turn_separator(self.transcript_preview.last().map(|entry| entry.role), role) {
            self.transcript_preview.push(TranscriptLine {
                role: TranscriptRole::Separator,
                text: TURN_SEPARATOR_LINE.to_string(),
                rendered: None,
            });
            if !self.transcript_follow_tail {
                self.transcript_scroll_lines_from_bottom =
                    self.transcript_scroll_lines_from_bottom.saturating_add(1);
            }
        }

        self.transcript_preview.push(TranscriptLine {
            role,
            text,
            rendered,
        });
        if self.transcript_follow_tail {
            self.transcript_cursor = self.transcript_preview.len().checked_sub(1);
        }
        if !self.transcript_follow_tail {
            self.transcript_scroll_lines_from_bottom =
                self.transcript_scroll_lines_from_bottom.saturating_add(1);
        }
    }

    pub fn scroll_transcript_page_up(&mut self, page_lines: usize) {
        self.transcript_follow_tail = false;
        let mut model = self.transcript_viewport_model();
        model.page_up(page_lines.max(1));
        self.apply_transcript_viewport_model(&model);
    }

    pub fn scroll_transcript_line_up(&mut self) {
        self.transcript_follow_tail = false;
        let mut model = self.transcript_viewport_model();
        model.line_up();
        self.apply_transcript_viewport_model(&model);
    }

    pub fn scroll_transcript_page_down(&mut self, page_lines: usize) {
        let mut model = self.transcript_viewport_model();
        model.page_down(page_lines.max(1));
        self.apply_transcript_viewport_model(&model);
    }

    pub fn scroll_transcript_line_down(&mut self) {
        let mut model = self.transcript_viewport_model();
        model.line_down();
        self.apply_transcript_viewport_model(&model);
    }

    pub fn scroll_transcript_to_top(&mut self) {
        let mut model = self.transcript_viewport_model();
        model.jump_top();
        self.apply_transcript_viewport_model(&model);
    }

    pub fn scroll_transcript_to_bottom(&mut self) {
        let mut model = self.transcript_viewport_model();
        model.jump_bottom();
        self.apply_transcript_viewport_model(&model);
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

    pub fn request_quit_if_idle(&mut self) {
        if self.phase == UiPhase::Idle && self.input.buffer.is_empty() {
            self.quit_requested = true;
        }
    }

    pub fn record_token_usage(&mut self, input_tokens: u64, output_tokens: u64, total_tokens: u64) {
        self.latest_input_tokens = Some(input_tokens);
        self.latest_output_tokens = Some(output_tokens);
        self.latest_total_tokens = Some(total_tokens);
        self.session_total_tokens = self.session_total_tokens.saturating_add(total_tokens);
    }

    pub fn ensure_invariants(&mut self) {
        if self.phase == UiPhase::AbortPending && !self.active_cycle {
            self.phase = UiPhase::Idle;
            self.abort.pending = false;
        }

        if self.phase != UiPhase::AbortPending {
            self.abort.pending = false;
        }

        if self.phase == UiPhase::Idle {
            self.active_cycle = false;
        }

        if self.input.cursor > self.input.buffer.len() {
            self.input.cursor = self.input.buffer.len();
        }

        self.clamp_scroll_from_bottom();

        while self.input.cursor > 0 && !self.input.buffer.is_char_boundary(self.input.cursor) {
            self.input.cursor -= 1;
        }

        let model = self.transcript_viewport_model();
        self.apply_transcript_viewport_model(&model);

        if let Some(selection) = self.visual_selection.as_ref()
            && selection.bounded_range(self.transcript_preview.len()).is_none()
        {
            self.visual_selection = None;
        }

        self.input.locked = self.phase != UiPhase::Idle;
    }

    fn max_scroll_from_bottom(&self) -> usize {
        let visible = self.transcript_viewport_lines.max(1);
        self.transcript_preview.len().saturating_sub(visible)
    }

    fn clamp_scroll_from_bottom(&mut self) {
        if self.transcript_follow_tail {
            self.transcript_scroll_lines_from_bottom = 0;
            return;
        }
        let max = self.max_scroll_from_bottom();
        if self.transcript_scroll_lines_from_bottom > max {
            self.transcript_scroll_lines_from_bottom = max;
        }
    }

    fn current_transcript_cursor_index(&self) -> Option<usize> {
        self.transcript_viewport_model().current_cursor_index()
    }

    fn transcript_viewport_model(&self) -> TranscriptViewport {
        let mut model = TranscriptViewport::new(
            self.transcript_preview.len(),
            self.transcript_viewport_lines.max(1),
        );
        model.set_cursor_index(self.transcript_cursor);
        if self.transcript_follow_tail {
            model.jump_bottom();
        } else {
            model.set_follow_tail_and_offset(false, self.transcript_scroll_lines_from_bottom);
        }
        model
    }

    fn apply_transcript_viewport_model(&mut self, model: &TranscriptViewport) {
        self.transcript_follow_tail = model.follow_tail();
        self.transcript_scroll_lines_from_bottom = model.offset_from_bottom();
        self.transcript_viewport_lines = model.viewport_lines();
        self.transcript_cursor = model.current_cursor_index();
    }
}

fn should_insert_turn_separator(previous: Option<TranscriptRole>, next: TranscriptRole) -> bool {
    matches!(
        (previous, next),
        (Some(prev), next) if is_turn_role(prev) && is_turn_role(next) && prev != next
    )
}

fn is_turn_role(role: TranscriptRole) -> bool {
    matches!(role, TranscriptRole::User | TranscriptRole::Assistant | TranscriptRole::Tool)
}

fn previous_char_start(buffer: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }

    let cursor = cursor.min(buffer.len());
    buffer[..cursor]
        .char_indices()
        .last()
        .map(|(idx, _)| idx)
}

fn next_char_end(buffer: &str, cursor: usize) -> Option<usize> {
    if cursor >= buffer.len() {
        return None;
    }

    let cursor = cursor.min(buffer.len());
    buffer[cursor..]
        .chars()
        .next()
        .map(|ch| cursor + ch.len_utf8())
}
