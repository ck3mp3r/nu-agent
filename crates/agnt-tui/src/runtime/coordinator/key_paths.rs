//! The coordinator's inline-slash and history key paths: the suggestion list's
//! keys and the submitted-prompt history walk.

use super::RuntimeCoordinator;
use crate::interaction::dispatch::{KeyLens, rewrite_action};
use crate::interaction::input::{Key, TerminalKey, UiInput};

impl RuntimeCoordinator {
    /// Handle keys when inline slash suggestions are open.
    /// Routes characters to TextArea, navigation to suggestion cycling,
    /// Enter/Esc to accept/close, and Backspace to delete + re-check.
    pub(super) fn handle_inline_slash_key(&mut self, key: TerminalKey) -> bool {
        match key {
            TerminalKey::Char(ch) => match rewrite_action(&mut self.state, Key::Char(ch)) {
                KeyLens::Pass => {
                    let _ = self.reduce_input(UiInput::KeyPressed { key: Key::Char(ch) });
                    self.textarea.input(ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Char(ch),
                        ctrl: false,
                        alt: false,
                        shift: false,
                    });
                    let buffer = self.textarea.lines().join("\n");
                    self.state.check_inline_slash(&buffer);
                    self.mark_render_needed();
                    true
                }
                KeyLens::Consumed(changed) => {
                    if changed {
                        self.mark_render_needed();
                    }
                    true
                }
                _ => false,
            },
            TerminalKey::Up | TerminalKey::BackTab => {
                if let Some(s) = self.state.picker.active_state_mut() {
                    s.move_up();
                }
                self.mark_render_needed();
                true
            }
            TerminalKey::Down => {
                if let Some(s) = self.state.picker.active_state_mut() {
                    s.move_down();
                }
                self.mark_render_needed();
                true
            }
            TerminalKey::Tab | TerminalKey::Enter => {
                let submit_key = if key == TerminalKey::Tab {
                    Key::Tab
                } else {
                    Key::Enter
                };
                let changed = self.reduce_input(UiInput::KeyPressed { key: submit_key });
                self.replace_textarea(Vec::new());
                self.state.check_inline_slash("");
                self.mark_render_needed();
                changed
            }
            TerminalKey::Esc => {
                // A running turn reroutes the press to the reducer. The slash
                // list closes there and the press ends at the surface: a
                // surface-close press never arms the abort.
                if self.state.phase != crate::state::UiPhase::Idle {
                    return false;
                }
                self.state.check_inline_slash("");
                self.mark_render_needed();
                true
            }
            TerminalKey::Backspace => {
                self.textarea.input(ratatui_textarea::Input {
                    key: ratatui_textarea::Key::Backspace,
                    ctrl: false,
                    alt: false,
                    shift: false,
                });
                let buffer = self.textarea.lines().join("\n");
                self.state.check_inline_slash(&buffer);
                self.mark_render_needed();
                true
            }
            _ => false,
        }
    }

    /// Navigate up in input history.
    /// Saves current textarea content, then loads history item into textarea.
    /// History navigation only applies while idle; the phase is
    /// orchestrator-owned, so the gate lives here, not in InputState.
    pub(super) fn handle_history_up(&mut self) -> bool {
        if self.state.phase != crate::state::UiPhase::Idle {
            return true;
        }
        let current = self.textarea.lines().join("\n");
        let submitted = self.state.submitted_prompt_texts();
        if let Some(text) = self.state.input.history_up(&submitted, &current) {
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            self.replace_textarea(lines);
            self.mark_render_needed();
        }
        true
    }

    /// Navigate down in input history.
    /// Loads next history item or restores saved draft into textarea.
    pub(super) fn handle_history_down(&mut self) -> bool {
        if self.state.phase != crate::state::UiPhase::Idle {
            return true;
        }
        let submitted = self.state.submitted_prompt_texts();
        if let Some(text) = self.state.input.history_down(&submitted) {
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            self.replace_textarea(lines);
            self.mark_render_needed();
        }
        true
    }
}
