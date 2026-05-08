use crossterm::cursor::SetCursorStyle;

use crate::agent::ui::tui::{
    rendering::selection::TranscriptSelection,
    state::AppState,
};

pub(super) fn build_status_lines(
    state: &AppState,
    active_model_identity: &str,
    _input_backend_status: &str,
    _last_input_poll_status: &str,
    _last_input_error: Option<&str>,
) -> Vec<String> {
    let (configured, enabled, disabled, failed) = state.mcp_counts();
    let model_phase = model_activity_label(state);

    let failure_line = format_mcp_failure_line(state, 64, 48, 100);

    vec![
        format!(
            "Model: {} ({model_phase})",
            ellipsize(active_model_identity, 60)
        ),
        format!(
            "MCP: configured={configured} enabled={enabled} disabled={disabled} failed={failed}"
        ),
        format!(
            "LLM-visible MCP tools: {}",
            state.llm_visible_mcp_tool_count()
        ),
        failure_line,
    ]
}

pub(super) fn compact_status_line(
    state: &AppState,
    active_model_identity: &str,
    _input_backend_status: &str,
    _last_input_poll_status: &str,
    _last_input_error: Option<&str>,
) -> String {
    let model_phase = model_activity_label(state);

    let (configured, enabled, disabled, failed) = state.mcp_counts();
    let failures = format_mcp_failure_line(state, 20, 20, 36);
    let failures_suffix = failures.trim_start_matches("Failures: ");

    let mut parts = Vec::new();
    parts.push(format!(
        "{} ({model_phase})",
        ellipsize(active_model_identity, 30)
    ));
    parts.push(format!(
        "mcp {configured}/{enabled}/{disabled}/{failed}"
    ));
    parts.push(format!("tools {}", state.llm_visible_mcp_tool_count()));
    parts.push(ellipsize(failures_suffix, 36));

    parts.join(" | ")
}

fn ellipsize(input: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let count = input.chars().count();
    if count <= max_chars {
        return input.to_string();
    }

    if max_chars == 1 {
        return "…".to_string();
    }

    let keep = max_chars - 1;
    let mut out = input.chars().take(keep).collect::<String>();
    out.push('…');
    out
}

fn model_activity_label(state: &AppState) -> &'static str {
    match state.phase {
        crate::agent::ui::tui::state::UiPhase::Busy
        | crate::agent::ui::tui::state::UiPhase::AbortPending => "busy",
        crate::agent::ui::tui::state::UiPhase::Idle => {
            if state.status_line == "Thinking..." || state.status_line.starts_with("Tool: ") {
                "busy"
            } else {
                "idle"
            }
        }
    }
}

fn format_mcp_failure_line(
    state: &AppState,
    max_name_chars: usize,
    max_reason_chars: usize,
    max_line_chars: usize,
) -> String {
    let failures = state.failed_mcp_servers_with_reasons();
    if failures.is_empty() {
        return "Failures: none (healthy)".to_string();
    }

    let joined = failures
        .into_iter()
        .map(|(name, reason)| match reason {
            Some(reason) => format!(
                "{} ({})",
                ellipsize(name, max_name_chars),
                ellipsize(reason, max_reason_chars)
            ),
            None => ellipsize(name, max_name_chars),
        })
        .collect::<Vec<_>>()
        .join(", ");

    format!("Failures: {}", ellipsize(&joined, max_line_chars))
}

pub(super) fn transcript_selection_for_render(state: &AppState) -> Option<TranscriptSelection> {
    if state.input_mode != crate::agent::ui::tui::state::InputMode::Visual {
        return None;
    }
    if state.pane_focus != crate::agent::ui::tui::state::PaneFocus::Transcript {
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

#[cfg(test)]
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
    mode: crate::agent::ui::tui::state::InputMode,
) -> SetCursorStyle {
    match mode {
        crate::agent::ui::tui::state::InputMode::Insert => SetCursorStyle::SteadyBar,
        crate::agent::ui::tui::state::InputMode::Normal
        | crate::agent::ui::tui::state::InputMode::Visual => SetCursorStyle::SteadyBlock,
    }
}

pub(super) fn availability_label(availability: Option<bool>) -> &'static str {
    match availability {
        Some(true) => "available",
        Some(false) => "unavailable",
        None => "unknown",
    }
}
