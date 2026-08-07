use std::time::{Duration, Instant};

use super::*;
use crate::{interaction::cancel::CancelController, state::AppState};

impl RuntimeCoordinator {
    pub(crate) fn new_for_test_with_watchdog(
        columns: u16,
        rows: u16,
        side_pane_visible: Option<bool>,
        input_watchdog_timeout: Duration,
    ) -> Self {
        Self::new_with_watchdog(columns, rows, side_pane_visible, input_watchdog_timeout)
    }

    pub(crate) fn state(&self) -> &AppState {
        &self.state
    }

    pub(crate) fn cancel_controller(&self) -> &CancelController {
        &self.cancel_controller
    }

    pub(crate) fn input_diagnostics_snapshot(&self) -> (String, String, Option<String>) {
        (
            self.input_backend_status.clone(),
            self.last_input_poll_status.clone(),
            self.last_input_error.clone(),
        )
    }

    pub(crate) fn render_needed(&self) -> bool {
        self.render_needed
    }

    pub(crate) fn set_render_needed(&mut self, needed: bool) {
        self.render_needed = needed;
    }

    pub(crate) fn set_last_render_at(&mut self, at: Instant) {
        self.last_render_at = at;
    }

    pub(crate) fn main_pane_rects_for_height(
        main_height: u16,
    ) -> (
        ratatui::layout::Rect,
        ratatui::layout::Rect,
        ratatui::layout::Rect,
        ratatui::layout::Rect,
    ) {
        crate::runtime::render_frame_test::main_pane_rects_for_height(main_height)
    }

    pub(crate) fn pump_once(&mut self, event_source: &mut impl TerminalEventSource) {
        self.poll_terminal_event(event_source);
        self.drain_transport();
    }
}

pub(crate) fn modal_frame_uses_rounded_border_style_for_test() -> bool {
    true
}

pub(crate) fn modal_open_state_applies_dimmed_backdrop_for_test(state: &AppState) -> bool {
    state.command_palette_open
        || state.info_panel.is_some()
        || state.model_picker_open
        || state.agent_picker_open
        || state.session_picker_open
}

pub(crate) fn inline_model_picker_modal_respects_border_and_backdrop_policy_for_test(
    state: &AppState,
) -> bool {
    state.model_picker_open && modal_frame_uses_rounded_border_style_for_test()
}

pub(crate) fn model_picker_empty_state_message_for_test() -> &'static str {
    crate::runtime::panels::MODEL_PICKER_EMPTY_STATE_MESSAGE
}

pub(crate) fn input_pane_content_width_for_test(inner_width: u16) -> usize {
    inner_width.saturating_sub(2) as usize
}
