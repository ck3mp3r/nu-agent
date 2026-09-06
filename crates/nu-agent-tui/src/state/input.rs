//! Input-mode domain state: the active [`InputMode`], j/k exit chords, the
//! pending `gg` key, input history navigation, pending submit/restore text,
//! and the clipboard copy request.

use super::*;
use crate::interaction::reducer::UserAction;

/// The vim-style input mode of the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Insert,
    Normal,
    Visual,
}

impl InputMode {
    pub fn cursor_style(&self) -> crossterm::cursor::SetCursorStyle {
        match self {
            InputMode::Insert => crossterm::cursor::SetCursorStyle::SteadyBar,
            InputMode::Normal | InputMode::Visual => crossterm::cursor::SetCursorStyle::SteadyBlock,
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

    // endregion: --- Mode transitions (input-domain part)

    // region:    --- Chord state

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

    // region:    --- Action rewriting

    /// Rewrite a user action through the input-mode lens: the vim keymap in
    /// normal/visual mode, j/k exit chords in insert mode, and the pending `gg`
    /// key. `idle` reports `UiPhase::Idle` — the phase is orchestrator-owned
    /// state, not input state, so the caller resolves it. Returns the
    /// (possibly rewritten) action and whether input state was modified.
    ///
    /// Escalation of a busy-mode Esc to `EscConfirm` (the abort-confirmation
    /// coupling) is resolved by the caller before this method sees the action:
    /// it needs `AppState::request_abort_confirmation`, which is lifecycle
    /// state, not input state.
    pub fn handle_action(&mut self, action: UserAction, idle: bool) -> (UserAction, bool) {
        if idle {
            return (self.handle_idle_action(action), false);
        }
        match self.mode {
            InputMode::Insert => self.handle_busy_insert_action(action),
            InputMode::Normal => self.handle_busy_normal_action(action),
            // Busy visual mode has no keymap of its own; chords reset and the
            // action passes through (a busy Esc is escalated by the caller).
            InputMode::Visual => {
                self.clear_insert_exit_pending_j();
                self.clear_normal_pending_key();
                (action, false)
            }
        }
    }

    /// Idle-mode rewriting: vim keymap in normal mode, visual-mode selection
    /// keys, and insert-mode chord tracking. Mode transitions happen in the
    /// reducer (via `EnterInsertMode`/`EnterNormalModeFromChord`/`Esc`), so no
    /// mode change occurs here.
    fn handle_idle_action(&mut self, action: UserAction) -> UserAction {
        match self.mode {
            InputMode::Normal => {
                self.clear_insert_exit_pending_j();
                match action {
                    UserAction::ToggleCommandPalette => {
                        self.clear_normal_pending_key();
                        UserAction::ToggleCommandPalette
                    }
                    UserAction::InsertChar('i') => {
                        self.clear_normal_pending_key();
                        UserAction::EnterInsertMode
                    }
                    UserAction::InsertChar('v') => {
                        self.clear_normal_pending_key();
                        UserAction::EnterVisualMode
                    }
                    UserAction::InsertChar('j') => {
                        self.clear_normal_pending_key();
                        UserAction::ScrollLineDown
                    }
                    UserAction::InsertChar('k') => {
                        self.clear_normal_pending_key();
                        UserAction::ScrollLineUp
                    }
                    UserAction::InsertChar('h') => {
                        self.clear_normal_pending_key();
                        UserAction::FocusPaneLeft
                    }
                    UserAction::InsertChar('l') => {
                        self.clear_normal_pending_key();
                        UserAction::FocusPaneRight
                    }
                    UserAction::CompleteForward => {
                        self.clear_normal_pending_key();
                        UserAction::FocusPaneRight
                    }
                    UserAction::CompleteBackward => {
                        self.clear_normal_pending_key();
                        UserAction::FocusPaneLeft
                    }
                    UserAction::InsertChar('g') => {
                        if self.take_normal_pending_key_if('g') {
                            UserAction::ScrollToTop
                        } else {
                            self.arm_normal_pending_key('g');
                            UserAction::Noop
                        }
                    }
                    UserAction::InsertChar('G') => {
                        self.clear_normal_pending_key();
                        UserAction::ScrollToBottom
                    }
                    UserAction::ScrollPageUp | UserAction::ScrollPageDown | UserAction::Esc => {
                        self.clear_normal_pending_key();
                        action
                    }
                    UserAction::InsertChar(_) => {
                        self.clear_normal_pending_key();
                        UserAction::Noop
                    }
                    UserAction::InsertNewline => {
                        self.clear_normal_pending_key();
                        UserAction::Noop
                    }
                    other => {
                        self.clear_normal_pending_key();
                        other
                    }
                }
            }
            InputMode::Visual => match action {
                UserAction::ToggleCommandPalette => {
                    self.clear_normal_pending_key();
                    UserAction::ToggleCommandPalette
                }
                UserAction::InsertChar('j') => {
                    self.clear_normal_pending_key();
                    UserAction::ScrollLineDown
                }
                UserAction::InsertChar('k') => {
                    self.clear_normal_pending_key();
                    UserAction::ScrollLineUp
                }
                UserAction::InsertChar('g') => {
                    if self.take_normal_pending_key_if('g') {
                        UserAction::ScrollToTop
                    } else {
                        self.arm_normal_pending_key('g');
                        UserAction::Noop
                    }
                }
                UserAction::InsertChar('G') => {
                    self.clear_normal_pending_key();
                    UserAction::ScrollToBottom
                }
                UserAction::InsertChar('y') => {
                    self.clear_normal_pending_key();
                    UserAction::YankSelection
                }
                UserAction::Esc => {
                    self.clear_normal_pending_key();
                    UserAction::Esc
                }
                UserAction::ScrollPageUp | UserAction::ScrollPageDown => {
                    self.clear_normal_pending_key();
                    action
                }
                UserAction::InsertNewline => {
                    self.clear_normal_pending_key();
                    UserAction::Noop
                }
                _ => {
                    self.clear_normal_pending_key();
                    UserAction::Noop
                }
            },
            InputMode::Insert => match action {
                UserAction::ToggleCommandPalette => {
                    self.clear_insert_exit_pending_j();
                    self.clear_normal_pending_key();
                    UserAction::ToggleCommandPalette
                }
                UserAction::InsertChar('j') => {
                    if self.insert_exit_pending_j() {
                        self.clear_insert_exit_pending_j();
                        UserAction::EnterNormalModeFromChord
                    } else {
                        self.set_insert_exit_pending_j();
                        UserAction::InsertChar('j')
                    }
                }
                UserAction::InsertChar('k') => {
                    if self.insert_exit_pending_j() {
                        self.clear_insert_exit_pending_j();
                        UserAction::EnterNormalModeFromChord
                    } else {
                        self.clear_insert_exit_pending_j();
                        UserAction::InsertChar('k')
                    }
                }
                UserAction::Esc => {
                    self.clear_insert_exit_pending_j();
                    self.clear_normal_pending_key();
                    UserAction::Esc
                }
                other => {
                    self.clear_insert_exit_pending_j();
                    self.clear_normal_pending_key();
                    other
                }
            },
        }
    }

    /// Busy insert-mode rewriting: j/k exit chords transition to normal mode
    /// directly (the reducer's chord arm only acts while idle), and Esc exits
    /// to normal mode and is escalated to the caller (abort-confirmation
    /// coupling).
    fn handle_busy_insert_action(&mut self, action: UserAction) -> (UserAction, bool) {
        match action {
            UserAction::ToggleCommandPalette => {
                self.clear_insert_exit_pending_j();
                self.clear_normal_pending_key();
                (UserAction::ToggleCommandPalette, false)
            }
            UserAction::InsertChar('j') => {
                if self.insert_exit_pending_j() {
                    self.clear_insert_exit_pending_j();
                    self.enter_normal_mode();
                    (UserAction::EnterNormalModeFromChord, true)
                } else {
                    self.set_insert_exit_pending_j();
                    (UserAction::InsertChar('j'), true) // force_changed: j/k chord tracking modifies state
                }
            }
            UserAction::InsertChar('k') => {
                if self.insert_exit_pending_j() {
                    self.clear_insert_exit_pending_j();
                    self.enter_normal_mode();
                    (UserAction::EnterNormalModeFromChord, true)
                } else {
                    self.clear_insert_exit_pending_j();
                    (UserAction::InsertChar('k'), true) // force_changed: j/k chord tracking modifies state
                }
            }
            UserAction::Esc => {
                self.clear_insert_exit_pending_j();
                self.clear_normal_pending_key();
                self.enter_normal_mode();
                (UserAction::Esc, true)
            }
            other => {
                self.clear_insert_exit_pending_j();
                self.clear_normal_pending_key();
                (other, false)
            }
        }
    }

    /// Busy normal-mode rewriting: the vim keymap applies during a running
    /// turn; `i` re-enters insert mode directly. Esc is escalated by the
    /// caller (abort-confirmation coupling) and passes through unchanged here.
    fn handle_busy_normal_action(&mut self, action: UserAction) -> (UserAction, bool) {
        match action {
            UserAction::InsertChar('i') => {
                self.clear_normal_pending_key();
                self.enter_insert_mode();
                (UserAction::Noop, true)
            }
            UserAction::InsertChar('j') => {
                self.clear_normal_pending_key();
                (UserAction::ScrollLineDown, false)
            }
            UserAction::InsertChar('k') => {
                self.clear_normal_pending_key();
                (UserAction::ScrollLineUp, false)
            }
            UserAction::InsertChar('h') => {
                self.clear_normal_pending_key();
                (UserAction::FocusPaneLeft, false)
            }
            UserAction::InsertChar('l') => {
                self.clear_normal_pending_key();
                (UserAction::FocusPaneRight, false)
            }
            UserAction::CompleteForward => {
                self.clear_normal_pending_key();
                (UserAction::FocusPaneRight, false)
            }
            UserAction::CompleteBackward => {
                self.clear_normal_pending_key();
                (UserAction::FocusPaneLeft, false)
            }
            UserAction::InsertChar('g') => {
                if self.take_normal_pending_key_if('g') {
                    (UserAction::ScrollToTop, false)
                } else {
                    self.arm_normal_pending_key('g');
                    (UserAction::Noop, false)
                }
            }
            UserAction::InsertChar('G') => {
                self.clear_insert_exit_pending_j();
                self.clear_normal_pending_key();
                (UserAction::ScrollToBottom, false)
            }
            UserAction::ScrollPageUp | UserAction::ScrollPageDown => {
                self.clear_insert_exit_pending_j();
                self.clear_normal_pending_key();
                (action, false)
            }
            UserAction::Esc => {
                self.clear_insert_exit_pending_j();
                self.clear_normal_pending_key();
                (UserAction::Esc, false)
            }
            UserAction::InsertChar(_) | UserAction::InsertNewline => {
                self.clear_insert_exit_pending_j();
                self.clear_normal_pending_key();
                (UserAction::Noop, false)
            }
            other => {
                self.clear_insert_exit_pending_j();
                self.clear_normal_pending_key();
                (other, false)
            }
        }
    }

    // endregion: --- Action rewriting
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
