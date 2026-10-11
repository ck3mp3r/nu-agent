#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TranscriptSelection {
    anchor: usize,
    cursor: usize,
}

impl TranscriptSelection {
    /// Starts a selection at the provided visual row.
    pub fn new(current_row: usize) -> Self {
        Self {
            anchor: current_row,
            cursor: current_row,
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

    pub fn move_cursor_down(&mut self, max_row: usize) {
        self.cursor = self.cursor.saturating_add(1).min(max_row);
    }

    pub fn move_cursor_to_top(&mut self) {
        self.cursor = 0;
    }

    pub fn move_cursor_to_bottom(&mut self, max_row: usize) {
        self.cursor = max_row;
    }

    /// Returns normalized inclusive bounds where start <= end (visual rows).
    pub fn normalized_range(&self) -> (usize, usize) {
        (self.anchor.min(self.cursor), self.anchor.max(self.cursor))
    }
}
