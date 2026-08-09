use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use ratatui::text::{Line, Span};

use crate::{rendering::theme::TuiTheme, state::AppState};

use super::RepoBranchTracker;

const BRANCH_ICON_PREFIX: &str = "\u{e725} ";
const BRANCH_ICON_PREFIX_WIDTH: usize = 2;

pub(crate) fn compact_status_line(
    active_model_identity: &str,
    repo_branch: Option<&str>,
    now_millis: Option<u128>,
    available_width: usize,
    theme: &TuiTheme,
) -> Line<'static> {
    format_lane_1(
        active_model_identity,
        repo_branch,
        now_millis,
        available_width,
        theme,
    )
}

impl RepoBranchTracker {
    pub(crate) fn from_caller_cwd_for_test(
        caller_cwd: Option<PathBuf>,
        watch_check_interval: Duration,
        fallback_poll_interval: Duration,
    ) -> Self {
        Self::from_caller_cwd_with_intervals(
            caller_cwd,
            watch_check_interval,
            fallback_poll_interval,
        )
    }
}

pub(crate) fn resolve_repo_branch_for_test(caller_cwd: &Path) -> Option<String> {
    let context = super::discover_git_repo_context(caller_cwd)?;
    let head_state = super::read_head_state(&context)?;
    head_state.branch_label()
}

pub(crate) fn lane_2_status_line(
    state: &AppState,
    available_width: usize,
    theme: &TuiTheme,
) -> Line<'static> {
    let current = state.latest_total_tokens.unwrap_or(0);
    let token_str = match state.context_window_max_tokens() {
        Some(max) if max > 0 => {
            let pct = ((current as u128).saturating_mul(100) / (max as u128)).min(100) as u64;
            format!("{} ({pct}%)", super::compact_token_count(current))
        }
        _ => super::compact_token_count(current),
    };
    match state.active_agent_identity().filter(|a| !a.is_empty()) {
        Some(agent) => {
            let left = if let Some(ref icon) = state.active_persona_icon {
                format!("{icon} {agent}")
            } else {
                agent.to_string()
            };
            let left_cells = 2 + 1 + agent.len();
            let right = &token_str;
            let padding = available_width.saturating_sub(left_cells + right.len());
            Line::from(vec![
                Span::styled(left, theme.role_assistant),
                Span::raw(" ".repeat(padding)),
                Span::styled(token_str, theme.subtle_meta),
            ])
        }
        None => align_right_lane_2_line(&token_str, available_width, theme),
    }
}

pub(crate) fn align_right_lane_2_line(
    line: &str,
    available_width: usize,
    theme: &TuiTheme,
) -> Line<'static> {
    let content = if line.chars().count() <= available_width {
        let pad = available_width.saturating_sub(line.chars().count());
        format!("{}{line}", " ".repeat(pad))
    } else {
        super::tail_ellipsize(line, available_width)
    };
    Line::from(vec![Span::styled(content, theme.subtle_meta)])
}

pub(crate) fn compact_status_line_with_branch_for_test(
    active_model_identity: &str,
    repo_branch: Option<&str>,
    now_millis: Option<u128>,
    available_width: usize,
) -> Line<'static> {
    compact_status_line(
        active_model_identity,
        repo_branch,
        now_millis,
        available_width,
        &TuiTheme::default(),
    )
}

pub(crate) fn format_lane_1(
    model: &str,
    repo_branch: Option<&str>,
    now_millis: Option<u128>,
    available_width: usize,
    theme: &TuiTheme,
) -> Line<'static> {
    let indicator = super::status_indicator(now_millis);
    let prefix_width = 2usize;
    let inner_width = available_width.saturating_sub(prefix_width);
    let display_model = model.to_string();

    let indicator_style = if now_millis.is_some() {
        theme.status_running
    } else {
        theme.status_done
    };

    match repo_branch.filter(|branch| !branch.is_empty()) {
        Some(branch) => {
            let (model_segment, padding_str, branch_segment) =
                format_lane_1_parts(&display_model, branch, inner_width);
            Line::from(vec![
                Span::styled(indicator.to_string(), indicator_style),
                Span::raw(" "),
                Span::styled(model_segment, theme.subtle_meta),
                Span::raw(padding_str),
                Span::styled(branch_segment, theme.focus),
            ])
        }
        None => {
            let model_segment = super::tail_ellipsize(&display_model, inner_width);
            Line::from(vec![
                Span::styled(indicator.to_string(), indicator_style),
                Span::raw(" "),
                Span::styled(model_segment, theme.subtle_meta),
            ])
        }
    }
}

pub(crate) fn format_lane_1_parts(
    model: &str,
    branch: &str,
    available_width: usize,
) -> (String, String, String) {
    let fields_max = available_width;

    if fields_max == 0 {
        return (String::new(), String::new(), String::new());
    }

    let gap_min = usize::from(fields_max > 1);
    let fields_budget = fields_max.saturating_sub(gap_min);

    let model_chars = model.chars().count();
    let branch_chars = branch.chars().count();
    let branch_display_chars = branch_chars.saturating_add(BRANCH_ICON_PREFIX_WIDTH);

    let (model_max, branch_max) = if model_chars + branch_display_chars <= fields_budget {
        (model_chars, branch_display_chars)
    } else {
        let model_only_budget = fields_budget.saturating_sub(branch_display_chars);
        if model_only_budget > 3 {
            (model_only_budget, branch_display_chars)
        } else {
            let branch_budget = fields_budget / 2;
            (fields_budget.saturating_sub(branch_budget), branch_budget)
        }
    };

    let model_segment = super::tail_ellipsize(model, model_max);
    let branch_segment = format_branch_segment(branch, branch_max);
    let padding = fields_budget
        .saturating_sub(model_segment.chars().count() + branch_segment.chars().count())
        + gap_min;

    (model_segment, " ".repeat(padding), branch_segment)
}

pub(crate) fn format_branch_segment(branch: &str, branch_max: usize) -> String {
    if branch_max <= BRANCH_ICON_PREFIX_WIDTH {
        return super::tail_ellipsize(branch, branch_max);
    }
    let label_budget = branch_max - BRANCH_ICON_PREFIX_WIDTH;
    let label = super::tail_ellipsize(branch, label_budget);
    format!("{BRANCH_ICON_PREFIX}{label}")
}

pub(crate) fn status_indicator_for_test(now_millis: Option<u128>) -> &'static str {
    super::status_indicator(now_millis)
}

#[test]
fn status_bar_uses_persona_icon_when_set() {
    let mut state = AppState::new();
    state.active_persona_icon = Some("🧠".to_string());
    state.set_active_agent_identity("test-agent");
    let line = super::status_left_content(
        "openai/gpt-4o-mini",
        None,
        &state,
        &TuiTheme::default(),
        120,
    );
    let joined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        joined.contains("🧠"),
        "status bar must use persona icon when set, got: {joined:?}"
    );
    assert!(
        !joined.contains("🪸"),
        "status bar must NOT use hardcoded emoji pool"
    );
}

#[test]
fn status_bar_no_icon_when_persona_icon_none() {
    let mut state = AppState::new();
    state.active_persona_icon = None;
    state.set_active_agent_identity("test-agent");
    let line = super::status_left_content(
        "openai/gpt-4o-mini",
        None,
        &state,
        &TuiTheme::default(),
        120,
    );
    let joined: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(
        !joined.contains("🧠"),
        "status bar must not show icon when none set"
    );
    assert!(
        !joined.contains("🪸"),
        "status bar must not use hardcoded emoji pool"
    );
}
