use std::path::Path;

use ratatui::text::{Line, Span};

use crate::{rendering::theme::TuiTheme, state::AppState};

use super::format::{compact_token_count, ellipsize, format_pwd, status_indicator, tail_ellipsize};

pub(crate) fn build_status_lines(state: &AppState) -> Vec<String> {
    let (configured, enabled, disabled, failed) = state.status.mcp.mcp_counts();
    let model_phase = model_activity_label(state);
    let active_model_identity = state.status.active_model_identity.as_str();

    let failure_line = format_mcp_failure_line(state, 64, 48, 100);

    let model_line = match state.status.active_agent_identity() {
        Some(agent) => format!(
            "Model: {} ({model_phase}) | agent: {agent}",
            ellipsize(active_model_identity, 60)
        ),
        None => format!(
            "Model: {} ({model_phase})",
            ellipsize(active_model_identity, 60)
        ),
    };

    vec![
        model_line,
        format!(
            "MCP: configured={configured} enabled={enabled} disabled={disabled} failed={failed}"
        ),
        format!(
            "LLM-visible MCP tools: {}",
            state.status.mcp.llm_visible_mcp_tool_count()
        ),
        failure_line,
    ]
}

fn token_string_for_state(state: &AppState) -> Option<String> {
    let current = state.status.latest_total_tokens.unwrap_or(0);
    let s = match state.status.context_window_max_tokens() {
        Some(max) if max > 0 => {
            let pct = ((current as u128).saturating_mul(100) / (max as u128)).min(100) as u64;
            format!("{} ({pct}%)", compact_token_count(current))
        }
        _ => compact_token_count(current),
    };
    Some(s)
}

pub(crate) fn status_left_content(
    busy_millis: Option<u128>,
    state: &AppState,
    theme: &TuiTheme,
    available_width: usize,
) -> Line<'static> {
    let model = state.status.active_model_identity.as_str();
    const SEP: &str = " ┃ ";
    const SEP_WIDTH: usize = 3;

    let indicator = status_indicator(busy_millis);
    let indicator_style = if busy_millis.is_some() {
        theme.status_running
    } else {
        theme.status_done
    };

    let prefix_width = 2usize;
    let budget = available_width.saturating_sub(prefix_width);

    let agent_opt = state
        .status
        .active_agent_identity()
        .filter(|a| !a.is_empty());
    let agent_str: Option<String> = agent_opt.map(|agent| {
        if let Some(icon) = &state.status.active_persona_icon {
            format!("{icon} {agent}")
        } else {
            agent.to_string()
        }
    });

    let token_str = token_string_for_state(state).unwrap_or_default();
    let token_segment_width = SEP_WIDTH + token_str.chars().count();

    let agent_width = agent_str.as_ref().map(|s| SEP_WIDTH + s.chars().count());

    let model_budget = {
        let after_tokens = budget.saturating_sub(token_segment_width);
        if let Some(aw) = agent_width {
            let min_model = 1usize;
            if after_tokens.saturating_sub(aw) >= min_model {
                after_tokens.saturating_sub(aw)
            } else {
                after_tokens
            }
        } else {
            after_tokens
        }
    };
    let model_display = tail_ellipsize(model, model_budget);
    let model_len_used = model_display.chars().count();

    let mut spans: Vec<Span<'static>> = Vec::new();
    spans.push(Span::styled(indicator.to_string(), indicator_style));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(model_display, theme.subtle_meta));

    if let Some(agent_display) = agent_str {
        let needed = prefix_width + model_len_used + SEP_WIDTH + agent_display.chars().count();
        if needed + token_segment_width <= available_width {
            spans.push(Span::styled(SEP.to_string(), theme.role_separator));
            spans.push(Span::styled(agent_display, theme.role_assistant));
        }
    }

    spans.push(Span::styled(SEP.to_string(), theme.role_separator));
    spans.push(Span::styled(token_str, theme.subtle_meta));

    Line::from(spans)
}

pub(crate) fn status_right_content(
    repo_branch: Option<&str>,
    cwd: Option<&Path>,
    theme: &TuiTheme,
) -> Option<Line<'static>> {
    const SEP: &str = " ┃ ";

    let branch_opt = repo_branch.filter(|b| !b.is_empty());
    let cwd_opt = cwd.map(format_pwd).filter(|s| !s.is_empty());

    if branch_opt.is_none() && cwd_opt.is_none() {
        return None;
    }

    let mut spans: Vec<Span<'static>> = Vec::new();

    if let Some(branch) = branch_opt {
        let branch_str = format!("\u{e725} {branch}");
        spans.push(Span::styled(branch_str, theme.focus));
    }

    if let Some(cwd_str) = cwd_opt {
        if branch_opt.is_some() {
            spans.push(Span::styled(SEP.to_string(), theme.role_separator));
        }
        spans.push(Span::styled(cwd_str, theme.subtle_meta));
    }

    Some(Line::from(spans))
}

pub(crate) fn model_activity_label(state: &AppState) -> &'static str {
    match state.phase {
        crate::state::UiPhase::Busy | crate::state::UiPhase::AbortPending => "busy",
        crate::state::UiPhase::Idle => {
            if state.status.status_line == "Thinking..."
                || state.status.status_line.starts_with("Tool: ")
                || state.compaction.in_progress()
            {
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
    let failures = state.status.mcp.failed_mcp_servers_with_reasons();
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

pub(crate) fn availability_label(availability: Option<bool>) -> &'static str {
    match availability {
        Some(true) => "available",
        Some(false) => "unavailable",
        None => "unknown",
    }
}
