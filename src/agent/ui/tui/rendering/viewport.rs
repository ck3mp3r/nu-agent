#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptViewport {
    transcript_len: usize,
    viewport_lines: usize,
    follow_tail: bool,
    offset_from_bottom: usize,
    cursor_index: Option<usize>,
}

impl TranscriptViewport {
    pub fn new(transcript_len: usize, viewport_lines: usize) -> Self {
        let mut model = Self {
            transcript_len,
            viewport_lines: viewport_lines.max(1),
            follow_tail: true,
            offset_from_bottom: 0,
            cursor_index: transcript_len.checked_sub(1),
        };
        model.normalize();
        model
    }

    pub fn transcript_len(&self) -> usize {
        self.transcript_len
    }

    pub fn viewport_lines(&self) -> usize {
        self.viewport_lines
    }

    pub fn follow_tail(&self) -> bool {
        self.follow_tail
    }

    pub fn offset_from_bottom(&self) -> usize {
        self.offset_from_bottom
    }

    pub fn max_offset_from_bottom(&self) -> usize {
        self.transcript_len.saturating_sub(self.viewport_lines)
    }

    pub fn current_cursor_index(&self) -> Option<usize> {
        self.cursor_index
    }

    pub fn set_cursor_index(&mut self, cursor_index: Option<usize>) {
        self.cursor_index = cursor_index;
        self.normalize();
    }

    pub fn set_follow_tail_and_offset(&mut self, follow_tail: bool, offset_from_bottom: usize) {
        self.follow_tail = follow_tail;
        self.offset_from_bottom = offset_from_bottom;
        self.normalize();
    }

    pub fn set_transcript_len(&mut self, transcript_len: usize) {
        self.transcript_len = transcript_len;
        self.normalize();
    }

    pub fn set_viewport_lines(&mut self, viewport_lines: usize) {
        self.viewport_lines = viewport_lines.max(1);
        self.normalize();
    }

    pub fn line_up(&mut self) {
        let Some(cursor) = self.cursor_index else {
            return;
        };
        if self.follow_tail {
            self.follow_tail = false;
            self.offset_from_bottom = 0;
        }
        self.cursor_index = Some(cursor.saturating_sub(1));
        self.normalize();
    }

    pub fn line_down(&mut self) {
        let Some(cursor) = self.cursor_index else {
            return;
        };
        let last = self.transcript_len.saturating_sub(1);
        self.cursor_index = Some(cursor.saturating_add(1).min(last));
        self.normalize();
    }

    pub fn page_up(&mut self, page_lines: usize) {
        let Some(cursor) = self.cursor_index else {
            return;
        };
        if self.follow_tail {
            self.follow_tail = false;
            self.offset_from_bottom = 0;
        }
        self.cursor_index = Some(cursor.saturating_sub(page_lines.max(1)));
        self.normalize();
    }

    pub fn page_down(&mut self, page_lines: usize) {
        let Some(cursor) = self.cursor_index else {
            return;
        };
        let last = self.transcript_len.saturating_sub(1);
        self.cursor_index = Some(cursor.saturating_add(page_lines.max(1)).min(last));
        self.normalize();
    }

    pub fn jump_top(&mut self) {
        if self.transcript_len == 0 {
            self.normalize();
            return;
        }

        self.cursor_index = Some(0);
        self.follow_tail = false;
        self.offset_from_bottom = self.max_offset_from_bottom();
        self.normalize();
    }

    pub fn jump_bottom(&mut self) {
        self.follow_tail = true;
        self.offset_from_bottom = 0;
        self.cursor_index = self.transcript_len.checked_sub(1);
        self.normalize();
    }

    fn normalize(&mut self) {
        self.viewport_lines = self.viewport_lines.max(1);
        if self.transcript_len == 0 {
            self.follow_tail = true;
            self.offset_from_bottom = 0;
            self.cursor_index = None;
            return;
        }

        let last = self.transcript_len - 1;
        let max_window_start = self.max_offset_from_bottom();
        let mut cursor = self.cursor_index.unwrap_or(last).min(last);

        let mut window_start = if self.follow_tail {
            max_window_start
        } else {
            max_window_start.saturating_sub(self.offset_from_bottom.min(max_window_start))
        };

        if cursor < window_start {
            window_start = cursor;
        }

        let mut window_end = window_start
            .saturating_add(self.viewport_lines.saturating_sub(1))
            .min(last);
        if cursor > window_end {
            window_start = cursor
                .saturating_add(1)
                .saturating_sub(self.viewport_lines)
                .min(max_window_start);
            window_end = window_start
                .saturating_add(self.viewport_lines.saturating_sub(1))
                .min(last);
        }

        if cursor > window_end {
            cursor = window_end;
        }

        let at_tail = cursor == last && window_start == max_window_start;
        if at_tail {
            self.follow_tail = true;
            self.offset_from_bottom = 0;
        } else {
            self.follow_tail = false;
            self.offset_from_bottom = max_window_start.saturating_sub(window_start);
        }

        self.cursor_index = Some(cursor);
    }
}
