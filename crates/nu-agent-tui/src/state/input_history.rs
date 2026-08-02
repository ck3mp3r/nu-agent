use super::{AppState, PromptStatus, UiPhase};

impl AppState {
    /// Navigate up in input history.
    /// Returns the history text to load, or None if no history is available.
    /// The caller is responsible for saving the current buffer before calling this.
    pub fn history_up(&mut self, current_buffer: &str) -> Option<String> {
        if self.phase != UiPhase::Idle {
            return None;
        }
        let submitted = self.submitted_prompt_texts();
        if submitted.is_empty() {
            return None;
        }
        if self.input_history_index.is_none() {
            self.input_history_saved = current_buffer.to_string();
        }
        let next = match self.input_history_index {
            None => submitted.len() - 1,
            Some(i) => i.saturating_sub(1),
        };
        self.input_history_index = Some(next);
        Some(submitted[next].clone())
    }

    /// Navigate down in input history.
    /// Returns Some(text) to load from history, or Some("") to restore the saved draft,
    /// or None if no history navigation is active.
    pub fn history_down(&mut self) -> Option<String> {
        if self.phase != UiPhase::Idle {
            return None;
        }
        let current = self.input_history_index?;
        let submitted = self.submitted_prompt_texts();
        if current + 1 >= submitted.len() {
            self.input_history_index = None;
            return Some(std::mem::take(&mut self.input_history_saved));
        }
        self.input_history_index = Some(current + 1);
        Some(submitted[current + 1].clone())
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
}
