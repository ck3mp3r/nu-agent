use super::{AppState, PromptStatus, UiPhase};

impl AppState {
    pub fn history_up(&mut self) {
        if self.phase != UiPhase::Idle {
            return;
        }
        if self.input.buffer[..self.input.cursor].contains('\n') {
            self.move_cursor_up_line();
            return;
        }
        let submitted = self.submitted_prompt_texts();
        if submitted.is_empty() {
            return;
        }
        if self.input_history_index.is_none() {
            self.input_history_saved = self.input.buffer.clone();
        }
        let next = match self.input_history_index {
            None => submitted.len() - 1,
            Some(i) => i.saturating_sub(1),
        };
        self.input_history_index = Some(next);
        self.input.buffer = submitted[next].clone();
        self.input.cursor = self.input.buffer.len();
        self.ensure_invariants();
    }

    pub fn history_down(&mut self) {
        if self.phase != UiPhase::Idle {
            return;
        }
        if let Some(current) = self.input_history_index {
            let submitted = self.submitted_prompt_texts();
            if current + 1 >= submitted.len() {
                self.input.buffer = std::mem::take(&mut self.input_history_saved);
                self.input.cursor = self.input.buffer.len();
                self.input_history_index = None;
            } else {
                self.input_history_index = Some(current + 1);
                self.input.buffer = submitted[current + 1].clone();
                self.input.cursor = self.input.buffer.len();
            }
            self.ensure_invariants();
            return;
        }
        if self.input.buffer[self.input.cursor..].contains('\n') {
            self.move_cursor_down_line();
        }
    }

    pub fn reset_history_navigation(&mut self) {
        self.input_history_index = None;
        self.input_history_saved.clear();
    }

    fn submitted_prompt_texts(&self) -> Vec<String> {
        self.prompt_items
            .iter()
            .filter(|p| p.status == PromptStatus::Done)
            .map(|p| p.prompt_text.clone())
            .collect()
    }

    fn move_cursor_up_line(&mut self) {
        let cursor = self.input.cursor;
        let buf = &self.input.buffer;
        let line_start = buf[..cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
        if line_start == 0 {
            return;
        }
        let col = cursor - line_start;
        let prev_end = line_start - 1;
        let prev_start = buf[..prev_end].rfind('\n').map(|i| i + 1).unwrap_or(0);
        self.input.cursor = prev_start + col.min(prev_end - prev_start);
        self.ensure_invariants();
    }

    fn move_cursor_down_line(&mut self) {
        let cursor = self.input.cursor;
        let buf = &self.input.buffer;
        let line_start = buf[..cursor].rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col = cursor - line_start;
        let next_start = match buf[cursor..].find('\n') {
            Some(off) => cursor + off + 1,
            None => return,
        };
        let next_end = buf[next_start..]
            .find('\n')
            .map(|i| next_start + i)
            .unwrap_or(buf.len());
        self.input.cursor = next_start + col.min(next_end - next_start);
        self.ensure_invariants();
    }
}
