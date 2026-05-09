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
    available_width: usize,
) -> String {
    compact_status_line_with_repo_branch(
        state,
        active_model_identity,
        current_repo_branch().as_deref(),
        available_width,
    )
}

pub(super) fn lane_2_status_line(state: &AppState, available_width: usize) -> String {
    let current = state.latest_total_tokens.unwrap_or(0);
    let line = match state.context_window_max_tokens() {
        Some(max) if max > 0 => {
            let pct = ((current as u128).saturating_mul(100) / (max as u128)).min(100) as u64;
            format!("{} ({pct}%)", compact_token_count(current))
        }
        _ => compact_token_count(current),
    };
    align_right_lane_2(&line, available_width)
}

fn align_right_lane_2(line: &str, available_width: usize) -> String {
    if line.chars().count() <= available_width {
        let pad = available_width.saturating_sub(line.chars().count());
        return format!("{}{line}", " ".repeat(pad));
    }

    tail_ellipsize(line, available_width)
}

fn compact_token_count(value: u64) -> String {
    if value < 1_000 {
        return value.to_string();
    }

    if value < 1_000_000 {
        return compact_scaled(value, 1_000, "k");
    }

    if value < 1_000_000_000 {
        return compact_scaled(value, 1_000_000, "M");
    }

    value.to_string()
}

fn compact_scaled(value: u64, divisor: u64, suffix: &str) -> String {
    let tenths = ((value as u128).saturating_mul(10) / (divisor as u128)) as u64;
    let whole = tenths / 10;
    let frac = tenths % 10;

    if frac == 0 {
        format!("{whole}{suffix}")
    } else {
        format!("{whole}.{frac}{suffix}")
    }
}

fn compact_status_line_with_repo_branch(
    _state: &AppState,
    active_model_identity: &str,
    repo_branch: Option<&str>,
    available_width: usize,
) -> String {
    format_lane_1(active_model_identity, repo_branch, available_width)
}

#[cfg(test)]
pub(super) fn compact_status_line_with_branch_for_test(
    state: &AppState,
    active_model_identity: &str,
    repo_branch: Option<&str>,
    available_width: usize,
) -> String {
    compact_status_line_with_repo_branch(state, active_model_identity, repo_branch, available_width)
}

fn format_lane_1(model: &str, repo_branch: Option<&str>, available_width: usize) -> String {
    match repo_branch.filter(|branch| !branch.is_empty()) {
        Some(branch) => format_lane_1_with_branch(model, branch, available_width),
        None => tail_ellipsize(model, available_width),
    }
}

fn format_lane_1_with_branch(model: &str, branch: &str, available_width: usize) -> String {
    let fields_max = available_width;

    if fields_max == 0 {
        return String::new();
    }

    // Keep a minimum visual gap between left and right segments when possible.
    let gap_min = usize::from(fields_max > 1);
    let fields_budget = fields_max.saturating_sub(gap_min);

    let model_chars = model.chars().count();
    let branch_chars = branch.chars().count();

    let (model_max, branch_max) = if model_chars + branch_chars <= fields_budget {
        (model_chars, branch_chars)
    } else {
        let model_only_budget = fields_budget.saturating_sub(branch_chars);
        if model_only_budget > 3 {
            (model_only_budget, branch_chars)
        } else {
            let branch_budget = fields_budget / 2;
            (fields_budget.saturating_sub(branch_budget), branch_budget)
        }
    };

    let model_segment = tail_ellipsize(model, model_max);
    let branch_segment = tail_ellipsize(branch, branch_max);
    let padding = fields_budget
        .saturating_sub(model_segment.chars().count() + branch_segment.chars().count())
        + gap_min;

    format!("{model_segment}{model_padding}{branch_segment}", model_padding = " ".repeat(padding))
}

fn tail_ellipsize(input: &str, max_chars: usize) -> String {
    let count = input.chars().count();
    if count <= max_chars {
        return input.to_string();
    }

    if max_chars == 0 {
        return String::new();
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let keep = max_chars - 3;
    let suffix = input
        .chars()
        .skip(count.saturating_sub(keep))
        .collect::<String>();
    format!("...{suffix}")
}

fn current_repo_branch() -> Option<String> {
    use std::sync::OnceLock;

    static REPO_BRANCH: OnceLock<Option<String>> = OnceLock::new();

    REPO_BRANCH
        .get_or_init(resolve_repo_branch)
        .clone()
}

fn resolve_repo_branch() -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["branch", "--show-current"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
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
