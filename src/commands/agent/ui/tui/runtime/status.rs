use crossterm::cursor::SetCursorStyle;

use crate::commands::agent::ui::tui::{
    rendering::selection::TranscriptSelection,
    state::AppState,
};

pub(super) fn build_status_lines(
    state: &AppState,
    active_model_identity: &str,
    input_backend_status: &str,
    last_input_poll_status: &str,
    last_input_error: Option<&str>,
) -> Vec<String> {
    let status = if state.status_line.is_empty() {
        match state.phase {
            crate::commands::agent::ui::tui::state::UiPhase::Idle => {
                "Idle (type and press Enter)"
            }
            crate::commands::agent::ui::tui::state::UiPhase::Busy => "Thinking...",
            crate::commands::agent::ui::tui::state::UiPhase::AbortPending => {
                crate::commands::agent::ui::tui::interaction::reducer::ESC_ABORT_CONFIRM_STATUS
            }
        }
    } else {
        &state.status_line
    };

    let input_error = last_input_error.unwrap_or("none");
    let tokens_line = format_tokens_line(state);
    let mode_line = match state.input_mode {
        crate::commands::agent::ui::tui::state::InputMode::Insert => {
            "Mode: INSERT (typing · Esc/jj/jk -> NORMAL)".to_string()
        }
        crate::commands::agent::ui::tui::state::InputMode::Normal => {
            "Mode: NORMAL (navigation · i INSERT · v VISUAL · h/l or Tab pane · j/k · gg/G)"
                .to_string()
        }
        crate::commands::agent::ui::tui::state::InputMode::Visual => {
            "Mode: VISUAL (transcript selection · j/k · gg/G · y yank · Esc)"
                .to_string()
        }
    };
    let focus_line = match state.pane_focus {
        crate::commands::agent::ui::tui::state::PaneFocus::Transcript => {
            "Focus: Transcript".to_string()
        }
        crate::commands::agent::ui::tui::state::PaneFocus::Input => "Focus: Input".to_string(),
    };
    let mut lines = vec![status.to_string(), mode_line, focus_line];

    if let Some(visual_line) = visual_indicator_line(state) {
        lines.push(visual_line);
    }

    lines.extend([
        tokens_line,
        format!("Model: {active_model_identity}"),
        format!("Input backend: {input_backend_status}"),
        format!("Input poll: {last_input_poll_status}"),
        format!("Input error: {input_error}"),
    ]);

    lines
}

pub(super) fn compact_status_line(
    state: &AppState,
    active_model_identity: &str,
    _input_backend_status: &str,
    _last_input_poll_status: &str,
    _last_input_error: Option<&str>,
) -> String {
    let status = if state.status_line.is_empty() {
        match state.phase {
            crate::commands::agent::ui::tui::state::UiPhase::Idle => "",
            crate::commands::agent::ui::tui::state::UiPhase::Busy => "busy",
            crate::commands::agent::ui::tui::state::UiPhase::AbortPending => "abort-pending",
        }
    } else {
        &state.status_line
    };

    let mode = match state.input_mode {
        crate::commands::agent::ui::tui::state::InputMode::Insert => "INS",
        crate::commands::agent::ui::tui::state::InputMode::Normal => "NOR",
        crate::commands::agent::ui::tui::state::InputMode::Visual => "VIS",
    };

    let tokens = match (
        state.latest_input_tokens,
        state.latest_output_tokens,
        state.latest_total_tokens,
    ) {
        (_, _, Some(_)) => format!("tokens: {}", state.session_total_tokens),
        _ => "tokens: n/a".to_string(),
    };

    let queue = state.pending_prompt_count();

    let mut parts = Vec::new();
    if !status.is_empty() {
        parts.push(status.to_string());
    }
    parts.push(mode.to_string());
    parts.push(format!("queue: {queue}"));
    parts.push(tokens);
    parts.push(active_model_identity.to_string());

    parts.join(" | ")
}

pub(super) fn transcript_selection_for_render(state: &AppState) -> Option<TranscriptSelection> {
    if state.input_mode != crate::commands::agent::ui::tui::state::InputMode::Visual {
        return None;
    }
    if state.pane_focus != crate::commands::agent::ui::tui::state::PaneFocus::Transcript {
        return None;
    }

    let (Some(anchor), Some(cursor)) = (state.visual_anchor_index(), state.visual_cursor_index())
    else {
        return None;
    };

    let mut selection = TranscriptSelection::new(anchor);
    selection.set_cursor(cursor);
    Some(selection)
}

pub(super) fn transcript_selection_range_for_render(
    state: &AppState,
    transcript_len: usize,
) -> Option<(usize, usize)> {
    transcript_selection_for_render(state).and_then(|selection| selection.bounded_range(transcript_len))
}

pub(super) fn transcript_title_for_render(state: &AppState, transcript_len: usize) -> String {
    let Some(selection) = transcript_selection_for_render(state) else {
        return "Transcript".to_string();
    };

    match selection.bounded_range(transcript_len) {
        Some((start, end)) => format!(
            "Transcript [VISUAL anchor={} cursor={} range={}..{}]",
            selection.anchor(),
            selection.cursor(),
            start,
            end
        ),
        None => "Transcript [VISUAL]".to_string(),
    }
}

pub(super) fn visual_indicator_line(state: &AppState) -> Option<String> {
    let selection = transcript_selection_for_render(state)?;
    let (start, end) = selection.normalized_range();
    Some(format!(
        "Visual: transcript anchor={} cursor={} range={}..{}",
        selection.anchor(),
        selection.cursor(),
        start,
        end
    ))
}

pub(super) fn cursor_style_for_mode(
    mode: crate::commands::agent::ui::tui::state::InputMode,
) -> SetCursorStyle {
    match mode {
        crate::commands::agent::ui::tui::state::InputMode::Insert => SetCursorStyle::SteadyBar,
        crate::commands::agent::ui::tui::state::InputMode::Normal
        | crate::commands::agent::ui::tui::state::InputMode::Visual => SetCursorStyle::SteadyBlock,
    }
}

pub(super) fn format_tokens_line(state: &AppState) -> String {
    match (
        state.latest_input_tokens,
        state.latest_output_tokens,
        state.latest_total_tokens,
    ) {
        (Some(input), Some(output), Some(total)) => format!(
            "Tokens: in={input} out={output} total={total} session={}",
            state.session_total_tokens
        ),
        _ => "Tokens: n/a".to_string(),
    }
}

pub(super) fn availability_label(availability: Option<bool>) -> &'static str {
    match availability {
        Some(true) => "available",
        Some(false) => "unavailable",
        None => "unknown",
    }
}
