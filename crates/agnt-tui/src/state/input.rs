//! Input-mode domain state: the active [`InputMode`], j/k exit chords, the
//! pending `gg` key, input history navigation, pending submit/restore text,
//! and the clipboard copy request.

use super::*;

/// The vim-style input mode of the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Insert,
    Normal,
    Visual,
}

/// The glyph the buffer-cell caret paints for an [`InputMode`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaretGlyph {
    /// Paint this glyph over the cell the caret occupies.
    Bar(char),
    /// Keep the character under the caret and reverse its style.
    Underlying,
}

impl InputMode {
    pub fn cursor_style(&self) -> crossterm::cursor::SetCursorStyle {
        match self {
            InputMode::Insert => crossterm::cursor::SetCursorStyle::SteadyBar,
            InputMode::Normal | InputMode::Visual => crossterm::cursor::SetCursorStyle::SteadyBlock,
        }
    }

    /// The buffer-cell caret's glyph: a hairline bar in insert mode, the
    /// character under the caret in normal/visual mode.
    pub fn caret_glyph(&self) -> CaretGlyph {
        match self {
            InputMode::Insert => CaretGlyph::Bar('▏'),
            InputMode::Normal | InputMode::Visual => CaretGlyph::Underlying,
        }
    }
}

/// Input-domain state extracted from `AppState`.
///
/// Every input-mode decision routes through this struct. Mode transitions
/// carried out here do not touch `PaneFocus` (owned by `AppState`); callers
/// sync the focus invariant via the [`AppState::enter_insert_mode`] and
/// [`AppState::enter_normal_mode`] wrappers.
#[derive(Debug, Clone)]
pub struct InputState {
    pub mode: InputMode,
    pub(super) history_index: Option<usize>,
    pub(super) history_saved: String,
    insert_exit_pending_j: Option<std::time::Instant>,
    /// How long an armed insert-exit `j` stays pending before it expires.
    insert_exit_expiry: std::time::Duration,
    normal_pending_key: Option<char>,
    pub pending_submit_text: Option<String>,
    /// Text restored from cancelled pending prompts, to be set on the textarea
    /// by the coordinator after the next pump cycle.
    pub restored_input_text: Option<String>,
    clipboard_request: Option<String>,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            mode: InputMode::Insert,
            history_index: None,
            history_saved: String::new(),
            insert_exit_pending_j: None,
            insert_exit_expiry: std::time::Duration::from_millis(500),
            normal_pending_key: None,
            pending_submit_text: None,
            restored_input_text: None,
            clipboard_request: None,
        }
    }
}

impl InputState {
    // region:    --- Mode transitions (input-domain part)

    /// Switch to insert mode and reset all pending chord state. Does not touch
    /// pane focus; see [`AppState::enter_insert_mode`].
    pub fn enter_insert_mode(&mut self) {
        self.mode = InputMode::Insert;
        self.insert_exit_pending_j = None;
        self.normal_pending_key = None;
    }

    /// Switch to normal mode and reset all pending chord state. Does not touch
    /// pane focus; see [`AppState::enter_normal_mode`].
    pub fn enter_normal_mode(&mut self) {
        self.mode = InputMode::Normal;
        self.insert_exit_pending_j = None;
        self.normal_pending_key = None;
    }

    /// Chainable: start from the default state with the given mode.
    pub fn with_mode(mut self, mode: InputMode) -> Self {
        self.mode = mode;
        self
    }

    /// Chainable: arm the pending submit text used by the next Submit.
    pub fn with_pending_submit_text(mut self, text: impl Into<String>) -> Self {
        self.pending_submit_text = Some(text.into());
        self
    }

    /// Chainable: set how long an armed insert-exit `j` stays pending before
    /// it expires. Production keeps the 500 ms default; tests inject a short
    /// window so the expiry is observable without a real-time wait.
    pub fn with_insert_exit_expiry(mut self, expiry: std::time::Duration) -> Self {
        self.insert_exit_expiry = expiry;
        self
    }

    // endregion: --- Mode transitions (input-domain part)

    // region:    --- Chord state

    pub fn set_insert_exit_pending_j(&mut self) {
        self.insert_exit_pending_j = Some(std::time::Instant::now());
    }

    pub fn clear_insert_exit_pending_j(&mut self) {
        self.insert_exit_pending_j = None;
    }

    /// Returns true only if pending AND within the expiry window.
    pub fn insert_exit_pending_j(&self) -> bool {
        match self.insert_exit_pending_j {
            Some(instant) => instant.elapsed() < self.insert_exit_expiry,
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

    // endregion: --- Chord state

    // region:    --- Clipboard request

    pub fn take_clipboard_request(&mut self) -> Option<String> {
        self.clipboard_request.take()
    }

    pub fn set_clipboard_request(&mut self, payload: String) {
        self.clipboard_request = Some(payload);
    }

    // endregion: --- Clipboard request

    // History navigation methods live in `input_history.rs` (impl InputState).
}

impl AppState {
    /// Enter insert mode: input-domain state plus the pane-focus invariant
    /// (insert mode focuses the input pane).
    pub fn enter_insert_mode(&mut self) {
        self.input.enter_insert_mode();
        self.scroll.pane_focus = PaneFocus::Input;
    }

    /// Enter normal mode: input-domain state plus the pane-focus invariant
    /// (normal mode focuses the transcript pane).
    pub fn enter_normal_mode(&mut self) {
        self.input.enter_normal_mode();
        self.scroll.pane_focus = PaneFocus::Transcript;
    }
}
