//! The coordinator's insert-mode key path: the textarea's own keys, the submit
//! arm, the history fallback, and the inline-slash hand-off.

use super::RuntimeCoordinator;
use crate::interaction::dispatch::{KeyLens, dispatch_terminal_event, rewrite_action};
use crate::interaction::input::{Key, TerminalEvent, TerminalKey, UiInput};
use crate::rendering::layout::input_content_row_count;
use crate::state::PickerRenderKind;

impl RuntimeCoordinator {
    /// Handle a key event in insert mode.
    /// Returns `true` if the event was consumed, `false` if it should fall through
    /// to the normal dispatch path.
    pub(super) fn handle_insert_mode_key(&mut self, key: TerminalKey) -> bool {
        // Info panel open? Route to dispatch so rewrite_action handles
        // info panel keys (j/k scrolling, Esc to close, etc.).
        if self.state.info_panel.is_some() {
            return false;
        }

        // Permission prompt active? Don't mutate textarea.
        if self.state.permission.has_prompt() {
            return false;
        }

        // Inline slash open? Handle keys directly (navigation, accept, close).
        if self.state.picker.render_kind() == Some(PickerRenderKind::InlineSlash) {
            return self.handle_inline_slash_key(key);
        }

        match key {
            // Submit — read textarea, clear it, dispatch submit
            TerminalKey::Enter => {
                let text = self.textarea.lines().join("\n");
                // Empty input while busy with queued prompts: cancel the running
                // turn so the existing drain path submits the queue.
                if text.is_empty()
                    && self.state.phase == crate::state::UiPhase::Busy
                    && self.state.pending_prompt_count() > 0
                {
                    self.cancel_controller.request_cancel();
                    self.mark_render_needed();
                    return true;
                }
                self.replace_textarea(Vec::new());
                self.state.input.pending_submit_text = Some(text);
                let changed = dispatch_terminal_event(
                    &mut self.state,
                    &TerminalEvent::Key(TerminalKey::Enter),
                    Some(&self.cancel_controller),
                );
                self.state.check_inline_slash("");
                self.mark_render_needed();
                changed
            }
            // Esc — let dispatch handle
            TerminalKey::Esc => false,
            // History — navigate within multiline textarea or history
            TerminalKey::Up => {
                if self.textarea.screen_cursor().row > 0 {
                    self.textarea.input(ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Up,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    });
                    self.mark_render_needed();
                    true
                } else {
                    self.handle_history_up()
                }
            }
            TerminalKey::Down => {
                let wrapped_row_count =
                    input_content_row_count(&self.textarea, self.last_input_content_width as u16);
                if self.textarea.screen_cursor().row + 1 < wrapped_row_count as usize {
                    self.textarea.input(ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Down,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    });
                    self.mark_render_needed();
                    true
                } else {
                    self.handle_history_down()
                }
            }
            // Navigation keys — route directly to TextArea
            TerminalKey::Left | TerminalKey::Right | TerminalKey::Home | TerminalKey::End => {
                let input = match key {
                    TerminalKey::Left => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Left,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                    TerminalKey::Right => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Right,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                    TerminalKey::Home => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Home,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                    TerminalKey::End => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::End,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                    // Unreachable: the outer guard restricts to the four arms above.
                    _ => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Null,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                };
                self.textarea.input(input);
                let buffer = self.textarea.lines().join("\n");
                self.state.check_inline_slash(&buffer);
                self.mark_render_needed();
                true
            }
            // Backspace/Delete — route directly to TextArea
            TerminalKey::Backspace | TerminalKey::Delete => {
                let input = match key {
                    TerminalKey::Backspace => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Backspace,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                    TerminalKey::Delete => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Delete,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                    // Unreachable: the outer guard restricts to the two arms above.
                    _ => ratatui_textarea::Input {
                        key: ratatui_textarea::Key::Null,
                        ctrl: false,
                        alt: false,
                        shift: false,
                    },
                };
                self.textarea.input(input);
                let buffer = self.textarea.lines().join("\n");
                self.state.check_inline_slash(&buffer);
                self.mark_render_needed();
                true
            }
            // AltEnter/ShiftEnter — insert newline directly
            TerminalKey::AltEnter | TerminalKey::ShiftEnter => {
                self.textarea.insert_newline();
                let buffer = self.textarea.lines().join("\n");
                self.state.check_inline_slash(&buffer);
                self.mark_render_needed();
                true
            }
            // All other keys — let dispatch handle them. CtrlC never reaches
            // here: the global quit guard intercepts it before routing.
            TerminalKey::PageUp
            | TerminalKey::PageDown
            | TerminalKey::CtrlU
            | TerminalKey::CtrlD
            | TerminalKey::CtrlP
            | TerminalKey::CtrlN
            | TerminalKey::CtrlJ
            | TerminalKey::CtrlK
            | TerminalKey::Tab
            | TerminalKey::BackTab
            | TerminalKey::CtrlC => false,
            // Char keys — the lens may consume them (panel, picker, the card),
            // the reducer's insert-exit chord may consume them, otherwise the
            // textarea takes them.
            TerminalKey::Char(ch) => {
                let chord_pending =
                    matches!(ch, 'j' | 'k') && self.state.input.insert_exit_pending_j();
                match rewrite_action(&mut self.state, Key::Char(ch)) {
                    KeyLens::Pass => {
                        let _ = self.reduce_input(UiInput::KeyPressed { key: Key::Char(ch) });
                        if chord_pending {
                            // The chord left insert mode: drop the j/k the textarea
                            // already holds.
                            if !self.state.input_locked {
                                self.textarea.input(ratatui_textarea::Input {
                                    key: ratatui_textarea::Key::Backspace,
                                    ctrl: false,
                                    alt: false,
                                    shift: false,
                                });
                            }
                            self.mark_render_needed();
                            true
                        } else {
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
                    }
                    KeyLens::Consumed(changed) => {
                        if changed {
                            self.mark_render_needed();
                        }
                        true
                    }
                    _ => false,
                }
            }
        }
    }
}
