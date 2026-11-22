//! The coordinator's terminal-event path: the Ctrl-C quit guard, the draft pickup,
//! the resize arm, and the re-wrap the layout owes a written draft.

use super::RuntimeCoordinator;
use crate::interaction::dispatch::dispatch_terminal_event;
use crate::interaction::input::{TerminalEvent, TerminalKey};
use crate::rendering::layout::{
    INPUT_MAX_HEIGHT, INPUT_MIN_HEIGHT, MAIN_SIDE_MARGIN, input_content_row_count,
};
use crate::state::{InputMode, PickerRenderKind};

impl RuntimeCoordinator {
    /// Process a single terminal event received from the input source: route
    /// insert-mode characters/paste into the textarea, and dispatch everything
    /// else (submit, quit, navigation, pickers) through the reducer. Called by
    /// the async render loop for every terminal event it receives.
    pub fn handle_terminal_event(&mut self, event: TerminalEvent) {
        // Pick up any restored input text from cancelled prompts before
        // processing the next event. A key, a paste, and a resize re-wrap the
        // draft in their own paths below, so the pickup leaves the re-wrap to
        // them; every other event carries the pickup alone, and the replaced
        // textarea's height must be computed here or it keeps the previous
        // draft's size.
        self.pickup_restored_input_text(!matches!(
            event,
            TerminalEvent::Key(_) | TerminalEvent::Paste(_) | TerminalEvent::Resize(_)
        ));

        if self.handle_ctrl_c_guard(&event) {
            return;
        }

        if let TerminalEvent::Resize(resize) = event {
            self.state.transcript.invalidate_height_index();
            let side_margin = if resize.columns >= 8 {
                MAIN_SIDE_MARGIN
            } else {
                0
            };
            self.last_input_content_width =
                resize.columns.saturating_sub(2 * side_margin + 4) as usize;
            self.recompute_layout_for_current_input();
            // The Resize reducer arm reports no change (it performs no
            // mutation), so the branch marks the layout change itself.
            self.mark_render_needed();
        }

        // A paste writes the textarea, so it obeys the same gates as typed
        // keys: the input lock drops it, and an open permission card owns the
        // input. A gated-off paste falls through to dispatch, which maps it to
        // no action and mutates nothing.
        if let TerminalEvent::Paste(text) = &event
            && self.state.input.mode == InputMode::Insert
            && (self.state.picker.active().is_none()
                || self.state.picker.render_kind() == Some(PickerRenderKind::InlineSlash))
            && self.state.info_panel.is_none()
            && !self.state.input_locked
            && !self.state.permission.has_prompt()
        {
            self.textarea.insert_str(text.as_str());
            let buffer = self.textarea.lines().join("\n");
            self.state.check_inline_slash(&buffer);
            self.mark_render_needed();
            self.recompute_layout_for_current_input();
            self.flush_clipboard_request();
            self.quit_requested |= self.state.quit_requested;
            self.sync_transcript_viewport_lines_with_layout();
            self.mark_render_needed();
            return;
        }

        let changed = if let TerminalEvent::Key(key) = event
            && self.state.input.mode == InputMode::Insert
            && (self.state.picker.active().is_none()
                || self.state.picker.render_kind() == Some(PickerRenderKind::InlineSlash))
            && self.state.info_panel.is_none()
        {
            let handled = self.handle_insert_mode_key(key);
            if handled {
                true
            } else {
                // Fall through to dispatch for keys not handled by TextArea
                // (CtrlC, Esc, CtrlP, CtrlN, Tab, etc.)
                dispatch_terminal_event(
                    &mut self.state,
                    &TerminalEvent::Key(key),
                    Some(&self.cancel_controller),
                )
            }
        } else {
            dispatch_terminal_event(&mut self.state, &event, Some(&self.cancel_controller))
        };

        if changed {
            self.mark_render_needed();
        }
        // Only a key or a paste can write the draft from here: a resize
        // re-wrapped it at the new content width above, and a wheel notch
        // scrolls the transcript without touching the draft. Re-wrapping an
        // untouched draft costs O(draft) per notch, so a long draft would make
        // every scroll step pay for text it cannot have changed.
        if matches!(event, TerminalEvent::Key(_) | TerminalEvent::Paste(_)) {
            self.recompute_layout_for_current_input();
        }
        self.flush_clipboard_request();
        self.quit_requested |= self.state.quit_requested;

        self.sync_transcript_viewport_lines_with_layout();
    }

    /// Global Ctrl-C quit guard. Returns `true` when the event was consumed by
    /// the guard and must not be routed further.
    ///
    /// Priority chain — each Ctrl-C press does exactly one thing:
    /// 1. permission prompt open → arm (the prompt keeps its own keys; Esc denies)
    /// 2. any other popup open → close it, no arm
    /// 3. no popup and non-empty draft → clear the draft, no arm
    /// 4. otherwise → arm
    ///
    /// The confirming press quits regardless of popups or draft state. Any
    /// other key, paste, or mouse event disarms; a resize leaves the armed
    /// state untouched.
    pub(crate) fn handle_ctrl_c_guard(&mut self, event: &TerminalEvent) -> bool {
        match event {
            TerminalEvent::Resize(_) => false,
            TerminalEvent::Paste(_) => {
                if self.state.disarm_quit_confirmation() {
                    self.mark_render_needed();
                }
                false
            }
            TerminalEvent::Mouse(_) => {
                if self.state.disarm_quit_confirmation() {
                    self.mark_render_needed();
                }
                false
            }
            TerminalEvent::Key(TerminalKey::CtrlC) => {
                self.handle_ctrl_c();
                true
            }
            TerminalEvent::Key(_) => {
                if self.state.disarm_quit_confirmation() {
                    self.mark_render_needed();
                }
                false
            }
        }
    }

    fn handle_ctrl_c(&mut self) {
        if self.state.quit_confirmation_armed() {
            self.state.disarm_quit_confirmation();
            self.state.quit_requested = true;
            self.quit_requested = true;
            if self.state.phase != crate::state::UiPhase::Idle {
                self.cancel_controller.request_cancel();
            }
            self.mark_render_needed();
            return;
        }

        if self.state.permission.has_prompt() {
            self.state.arm_quit_confirmation();
            self.mark_render_needed();
            return;
        }

        if self.state.picker.active().is_some() {
            self.state.picker.close();
            self.mark_render_needed();
            return;
        }

        if self.state.info_panel.is_some() {
            self.state.close_info_panel();
            self.mark_render_needed();
            return;
        }

        if !self.textarea.lines().join("\n").is_empty() {
            self.replace_textarea(Vec::new());
            self.state.check_inline_slash("");
            self.recompute_layout_for_current_input();
            self.mark_render_needed();
            return;
        }

        self.state.arm_quit_confirmation();
        self.mark_render_needed();
    }

    /// Apply any restored input text from cancelled prompts to the textarea.
    /// Idempotent via `take()`: safe to call from both the sync poll path and
    /// the async render loop's terminal arm.
    ///
    /// `recompute_layout` says whether the event that carries the pickup re-wraps
    /// the draft itself: a key, a paste, and a resize do, a wheel notch does not.
    /// The replaced textarea inherits the previous draft's wrapped height, so the
    /// event that writes it must re-wrap it — once, from whichever side owns it.
    fn pickup_restored_input_text(&mut self, recompute_layout: bool) {
        if let Some(text) = self.state.input.restored_input_text.take() {
            let lines: Vec<String> = text.lines().map(|l| l.to_string()).collect();
            let last_line = lines.len().saturating_sub(1) as u16;
            let last_col = lines.last().map(|l| l.len()).unwrap_or(0) as u16;
            self.replace_textarea(lines);
            self.textarea
                .move_cursor(ratatui_textarea::CursorMove::Jump(last_line, last_col));
            if recompute_layout {
                self.recompute_layout_for_current_input();
            }
            self.mark_render_needed();
        }
    }

    fn recompute_layout_for_current_input(&mut self) {
        self.layout_recomputes = self.layout_recomputes.saturating_add(1);
        self.input_height =
            input_content_row_count(&self.textarea, self.last_input_content_width as u16)
                .clamp(INPUT_MIN_HEIGHT, INPUT_MAX_HEIGHT);
    }
}
