use super::AppState;
use crate::agent::ui::tui::rendering::viewport::TranscriptViewport;

pub(super) fn max_scroll_from_bottom(state: &AppState) -> usize {
    let visible = state.transcript_viewport_lines.max(1);
    state.transcript_preview.len().saturating_sub(visible)
}

pub(super) fn clamp_scroll_from_bottom(state: &mut AppState) {
    if state.transcript_follow_tail {
        state.transcript_scroll_lines_from_bottom = 0;
        return;
    }
    let max = max_scroll_from_bottom(state);
    if state.transcript_scroll_lines_from_bottom > max {
        state.transcript_scroll_lines_from_bottom = max;
    }
}

pub(super) fn current_transcript_cursor_index(state: &AppState) -> Option<usize> {
    transcript_viewport_model(state).current_cursor_index()
}

pub(super) fn transcript_viewport_model(state: &AppState) -> TranscriptViewport {
    let mut model =
        TranscriptViewport::new(state.transcript_preview.len(), state.transcript_viewport_lines.max(1));
    model.set_cursor_index(state.transcript_cursor);
    if state.transcript_follow_tail {
        model.jump_bottom();
    } else {
        model.set_follow_tail_and_offset(false, state.transcript_scroll_lines_from_bottom);
    }
    model
}

pub(super) fn apply_transcript_viewport_model(state: &mut AppState, model: &TranscriptViewport) {
    state.transcript_follow_tail = model.follow_tail();
    state.transcript_scroll_lines_from_bottom = model.offset_from_bottom();
    state.transcript_viewport_lines = model.viewport_lines();
    state.transcript_cursor = model.current_cursor_index();
}
