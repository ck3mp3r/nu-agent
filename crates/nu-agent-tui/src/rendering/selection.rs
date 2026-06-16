#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptSelection {
    anchor: usize,
    cursor: usize,
}

impl TranscriptSelection {
    /// Starts a transcript-only selection at the current transcript cursor index.
    pub fn new(current_transcript_index: usize) -> Self {
        Self {
            anchor: current_transcript_index,
            cursor: current_transcript_index,
        }
    }

    pub fn anchor(&self) -> usize {
        self.anchor
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor;
    }

    pub fn move_cursor_up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn move_cursor_down(&mut self, max_index: usize) {
        self.cursor = self.cursor.saturating_add(1).min(max_index);
    }

    pub fn move_cursor_to_top(&mut self) {
        self.cursor = 0;
    }

    pub fn move_cursor_to_bottom(&mut self, max_index: usize) {
        self.cursor = max_index;
    }

    /// Returns normalized inclusive bounds where start <= end.
    pub fn normalized_range(&self) -> (usize, usize) {
        (self.anchor.min(self.cursor), self.anchor.max(self.cursor))
    }

    /// Returns transcript-bounded inclusive selection range.
    pub fn bounded_range(&self, transcript_len: usize) -> Option<(usize, usize)> {
        if transcript_len == 0 {
            return None;
        }

        let (start, end) = self.normalized_range();
        if start >= transcript_len {
            return None;
        }

        Some((start, end.min(transcript_len.saturating_sub(1))))
    }

    /// Builds yank payload from transcript lines in the bounded selection range.
    /// Returns empty string for empty transcript or out-of-range selection.
    pub fn yank_payload<S: AsRef<str>>(&self, transcript_lines: &[S]) -> String {
        let Some((start, end)) = self.bounded_range(transcript_lines.len()) else {
            return String::new();
        };

        transcript_lines[start..=end]
            .iter()
            .map(AsRef::as_ref)
            .collect::<Vec<_>>()
            .join("\n")
    }
}
