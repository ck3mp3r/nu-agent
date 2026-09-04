//! Input history navigation on [`InputState`].

use super::InputState;

impl InputState {
    /// Navigate up in input history.
    ///
    /// `submitted` is the completed-prompt history, oldest first. Returns the
    /// history text to load, or None if no history is available. The caller is
    /// responsible for saving the current buffer before calling this and for
    /// gating on `UiPhase::Idle` (the phase is orchestrator-owned).
    pub fn history_up(&mut self, submitted: &[String], current_buffer: &str) -> Option<String> {
        if submitted.is_empty() {
            return None;
        }
        if self.history_index.is_none() {
            self.history_saved = current_buffer.to_string();
        }
        let next = match self.history_index {
            None => submitted.len() - 1,
            Some(i) => i.saturating_sub(1),
        };
        self.history_index = Some(next);
        Some(submitted[next].clone())
    }

    /// Navigate down in input history.
    /// Returns Some(text) to load from history, or Some("") to restore the saved draft,
    /// or None if no history navigation is active.
    pub fn history_down(&mut self, submitted: &[String]) -> Option<String> {
        let current = self.history_index?;
        if current + 1 >= submitted.len() {
            self.history_index = None;
            return Some(std::mem::take(&mut self.history_saved));
        }
        self.history_index = Some(current + 1);
        Some(submitted[current + 1].clone())
    }

    pub fn reset_history_navigation(&mut self) {
        self.history_index = None;
        self.history_saved.clear();
    }
}
