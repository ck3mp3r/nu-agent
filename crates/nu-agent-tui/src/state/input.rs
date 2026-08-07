use super::*;

impl AppState {
    pub fn enter_insert_mode(&mut self) {
        self.input_mode = InputMode::Insert;
        self.insert_exit_pending_j = None;
        self.normal_pending_key = None;
        self.pane_focus = PaneFocus::Input;
    }

    pub fn enter_normal_mode(&mut self) {
        self.input_mode = InputMode::Normal;
        self.insert_exit_pending_j = None;
        self.normal_pending_key = None;
        self.pane_focus = PaneFocus::Transcript;
    }

    pub fn set_insert_exit_pending_j(&mut self) {
        self.insert_exit_pending_j = Some(std::time::Instant::now());
    }

    pub fn clear_insert_exit_pending_j(&mut self) {
        self.insert_exit_pending_j = None;
    }

    /// Returns true only if pending AND within the timeout window.
    pub fn insert_exit_pending_j(&self) -> bool {
        match self.insert_exit_pending_j {
            Some(instant) => instant.elapsed() < std::time::Duration::from_millis(500),
            None => false,
        }
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
}
