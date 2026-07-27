use super::*;

impl AppState {
    pub fn append_input_char(&mut self, ch: char) {
        self.reset_history_navigation();
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

    pub fn insert_input_newline(&mut self) {
        self.append_input_char('\n');
    }

    pub fn enter_insert_mode(&mut self) {
        self.input_mode = InputMode::Insert;
        self.insert_exit_pending_j = false;
        self.normal_pending_key = None;
        self.pane_focus = PaneFocus::Input;
    }

    pub fn enter_normal_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.insert_exit_pending_j = false;
        self.normal_pending_key = None;
        self.pane_focus = PaneFocus::Transcript;
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

    pub fn take_clipboard_request(&mut self) -> Option<String> {
        self.clipboard_request.take()
    }

    pub fn set_clipboard_request(&mut self, payload: String) {
        self.clipboard_request = Some(payload);
    }

    pub fn backspace_input_char(&mut self) {
        self.reset_history_navigation();
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
}

fn previous_char_start(buffer: &str, cursor: usize) -> Option<usize> {
    if cursor == 0 {
        return None;
    }

    let cursor = cursor.min(buffer.len());
    buffer[..cursor].char_indices().last().map(|(idx, _)| idx)
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
