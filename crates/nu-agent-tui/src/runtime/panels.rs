use super::*;

pub(super) fn transcript_line_status_to_item_status(
    status: TranscriptLineStatus,
) -> nu_agent_core::transcript::renderer::ItemStatus {
    use crate::state::{CompactionStatus, PromptStatus, ToolCallStatus};
    use nu_agent_core::transcript::renderer::ItemStatus;
    match status {
        TranscriptLineStatus::Tool(ToolCallStatus::InProgress) => ItemStatus::InProgress,
        TranscriptLineStatus::Tool(ToolCallStatus::Done) => ItemStatus::Done,
        TranscriptLineStatus::Tool(ToolCallStatus::Failed) => ItemStatus::Failed,
        TranscriptLineStatus::Prompt(PromptStatus::InProgress) => ItemStatus::InProgress,
        TranscriptLineStatus::Prompt(PromptStatus::Done) => ItemStatus::Done,
        TranscriptLineStatus::Prompt(PromptStatus::Cancelled) => ItemStatus::Cancelled,
        TranscriptLineStatus::Prompt(PromptStatus::Queued) => ItemStatus::Queued,
        TranscriptLineStatus::Compaction(CompactionStatus::InProgress) => ItemStatus::InProgress,
        TranscriptLineStatus::Compaction(CompactionStatus::Done) => ItemStatus::Done,
        TranscriptLineStatus::Compaction(CompactionStatus::Failed) => ItemStatus::Failed,
    }
}

pub(super) fn render_permission_controls(frame: &mut ratatui::Frame, area: Rect, theme: &TuiTheme) {
    let controls = Line::from(vec![
        Span::styled("[a]", theme.status_running),
        Span::raw(" allow once  "),
        Span::styled("[A]", theme.status_running),
        Span::raw(" allow always  "),
        Span::styled("[d/Esc]", theme.status_running),
        Span::raw(" deny"),
    ]);
    let widget = Paragraph::new(Text::from(vec![controls]));
    frame.render_widget(widget, area);
}

pub(super) fn transcript_entries_for_render(state: &AppState) -> &[TranscriptEntry] {
    &state.transcript_preview
}

pub(super) fn transcript_line_statuses_for_render(
    state: &AppState,
    entries: &[TranscriptEntry],
) -> Vec<Option<TranscriptLineStatus>> {
    entries
        .iter()
        .enumerate()
        .map(|(idx, entry)| {
            if matches!(entry, TranscriptEntry::Spacer(_)) {
                None
            } else {
                state.transcript_line_status_for_index(idx)
            }
        })
        .collect()
}

pub(super) fn wrapped_visual_rows_for_rendered_line(
    rendered_line: &Line<'_>,
    content_width: usize,
) -> usize {
    let width = rendered_line
        .spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum::<usize>()
        .max(1);
    width.div_ceil(content_width.max(1))
}

pub(super) fn help_panel_total_visual_rows(lines: &[Line<'_>], content_width: usize) -> usize {
    lines
        .iter()
        .map(|line| wrapped_visual_rows_for_rendered_line(line, content_width.max(1)))
        .sum()
}

pub(super) fn help_panel_max_scroll(
    lines: &[Line<'_>],
    viewport_height: u16,
    content_width: u16,
) -> usize {
    let visible_rows = viewport_height.max(1) as usize;
    let total_rows = help_panel_total_visual_rows(lines, content_width.max(1) as usize);
    total_rows.saturating_sub(visible_rows)
}

pub(super) fn help_panel_overflow_cue(
    lines: &[Line<'_>],
    viewport_height: u16,
    content_width: u16,
    scroll: usize,
) -> Option<String> {
    let visible_rows = viewport_height.max(1) as usize;
    let total_rows = help_panel_total_visual_rows(lines, content_width.max(1) as usize);
    if total_rows <= visible_rows {
        return None;
    }

    let max_scroll = total_rows.saturating_sub(visible_rows);
    let current = scroll.min(max_scroll);
    let start = current.saturating_add(1);
    let end = current.saturating_add(visible_rows).min(total_rows);
    Some(format!(
        "PgUp/PgDn j/k | Esc close | {}-{} / {}",
        start, end, total_rows
    ))
}

pub(super) fn command_palette_action_keys() -> &'static str {
    "↑/↓ or Ctrl-N · Enter · Esc"
}

pub(super) fn command_palette_title(overflow_cue: Option<&str>) -> String {
    let base = "Command Palette";
    let global_hint = command_palette_action_keys();
    if let Some(cue) = overflow_cue {
        format!("{base} ({global_hint} | {cue})")
    } else {
        format!("{base} ({global_hint})")
    }
}

pub(super) const MODEL_PICKER_EMPTY_STATE_MESSAGE: &str =
    "No models available in cached startup config.";
pub(super) const AGENT_PICKER_EMPTY_STATE_MESSAGE: &str =
    "No agent personas found. Create .agents/<name>.md files.";

pub(crate) fn skills_panel_lines(state: &AppState) -> (&'static str, Vec<Line<'static>>) {
    if state.skills_discovery_failed() {
        return (
            "Skills",
            vec![Line::from(
                "Skills discovery unavailable. Showing no skills.",
            )],
        );
    }

    if state.discoverable_skills().is_empty() {
        return (
            "Skills",
            vec![Line::from("No discoverable skills available.")],
        );
    }

    let mut lines = vec![Line::from("Discoverable skills")];
    lines.extend(
        state
            .discoverable_skills()
            .iter()
            .map(|skill| Line::from(format!("- {} ({})", skill.name, skill.source))),
    );
    ("Skills", lines)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommandPaletteTableModel {
    pub(super) query_line: String,
    pub(super) columns: Vec<String>,
    pub(super) rows: Vec<[String; 3]>,
    pub(super) selected: Option<usize>,
    pub(super) overflow_cue: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct McpTableModel {
    pub(super) columns: Vec<String>,
    pub(super) rows: Vec<[String; 3]>,
    pub(super) selected: Option<usize>,
    pub(super) overflow_cue: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct McpSelectedDetails {
    pub(super) server_line: String,
    pub(super) error_line: String,
    pub(super) tools_line: String,
    pub(super) tool_names: Vec<String>,
    pub(super) compact_single_line: String,
}

pub(crate) const MCP_STATUS_COLUMN_WIDTH: u16 = 6;

pub(super) fn mcp_selected_details(state: &AppState) -> Option<McpSelectedDetails> {
    let server = state.mcp_servers.get(state.mcp_panel_selection)?;
    let reason = state
        .failed_mcp_servers_with_reasons()
        .into_iter()
        .find(|(name, _)| *name == server.name.as_str())
        .and_then(|(_, reason)| reason)
        .map(str::trim)
        .filter(|reason| !reason.is_empty());
    let mut tool_names = state.mcp_visible_tool_names_for_server_name(server.name.as_str());
    tool_names.sort();
    tool_names.dedup();
    let tools_line = if tool_names.is_empty() {
        "Tools: None".to_string()
    } else {
        format!("Tools: {}", tool_names.join(", "))
    };
    let server_line = format!("Server: {} ({})", server.name, server.state.label());
    Some(McpSelectedDetails {
        compact_single_line: format!(
            "{} · {server_line}",
            match reason {
                Some(full) => format!("Error: {full}"),
                None => "Error: None".to_string(),
            }
        ),
        server_line,
        tools_line,
        tool_names,
        error_line: match reason {
            Some(full) => format!("Error: {full}"),
            None => "Error: None".to_string(),
        },
    })
}

pub(super) fn mcp_tool_lines_wrapped(
    tool_names: &[String],
    max_lines: usize,
    width: usize,
) -> Vec<String> {
    const PREFIX: &str = "Tools: ";
    let continuation_prefix = " ".repeat(PREFIX.chars().count());
    let content_width = width.saturating_sub(PREFIX.chars().count());

    if max_lines == 0 {
        return Vec::new();
    }

    if tool_names.is_empty() {
        return vec![format!("{PREFIX}None")];
    }

    let mut wrapped_groups: Vec<Vec<&str>> = Vec::new();
    for tool_name in tool_names {
        let tool_name = tool_name.as_str();
        if let Some(last) = wrapped_groups.last_mut() {
            let current_width = last.iter().map(|tool| tool.chars().count()).sum::<usize>()
                + (last.len().saturating_sub(1) * 2);
            let appended_width = current_width + 2 + tool_name.chars().count();
            if appended_width <= content_width || last.is_empty() {
                last.push(tool_name);
                continue;
            }
        }
        wrapped_groups.push(vec![tool_name]);
    }

    let mut lines: Vec<String> = Vec::new();
    if wrapped_groups.len() <= max_lines {
        for (idx, group) in wrapped_groups.into_iter().enumerate() {
            let prefix = if idx == 0 {
                PREFIX
            } else {
                continuation_prefix.as_str()
            };
            lines.push(format!("{prefix}{}", group.join(", ")));
        }
        return lines;
    }

    let leading_visible_lines = max_lines.saturating_sub(1);
    let mut visible_tool_count = 0usize;
    for (idx, group) in wrapped_groups
        .iter()
        .take(leading_visible_lines)
        .enumerate()
    {
        let prefix = if idx == 0 {
            PREFIX
        } else {
            continuation_prefix.as_str()
        };
        lines.push(format!("{prefix}{}", group.join(", ")));
        visible_tool_count = visible_tool_count.saturating_add(group.len());
    }

    let remaining = &tool_names[visible_tool_count.min(tool_names.len())..];
    let final_prefix = if lines.is_empty() {
        PREFIX
    } else {
        continuation_prefix.as_str()
    };

    let mut final_line = format!("+{} more", remaining.len());
    for visible in (0..remaining.len()).rev() {
        let hidden = remaining.len().saturating_sub(visible);
        if hidden == 0 {
            continue;
        }
        let candidate = if visible == 0 {
            format!("+{hidden} more")
        } else {
            format!("{}, +{hidden} more", remaining[..visible].join(", "))
        };
        if candidate.chars().count() <= content_width {
            final_line = candidate;
            break;
        }
    }

    lines.push(format!("{final_prefix}{final_line}"));
    lines.truncate(max_lines);
    lines
}

pub(super) fn mcp_selected_details_lines(
    state: &AppState,
    details_height: u16,
    details_width: u16,
) -> Vec<Line<'static>> {
    let Some(details) = mcp_selected_details(state) else {
        return Vec::new();
    };

    match details_height {
        0 => Vec::new(),
        1 => vec![Line::from(details.compact_single_line)],
        2 => vec![
            Line::from(details.server_line),
            Line::from(details.error_line),
        ],
        _ => {
            let mut lines = vec![
                Line::from(details.server_line),
                Line::from(details.error_line),
            ];
            if details.tool_names.is_empty() {
                lines.push(Line::from("Tools: None"));
                lines.truncate(details_height as usize);
                return lines;
            }

            let tool_line_budget = details_height.saturating_sub(2) as usize;
            if tool_line_budget == 0 {
                return lines;
            }

            let wrapped_tools = mcp_tool_lines_wrapped(
                &details.tool_names,
                tool_line_budget,
                details_width as usize,
            );
            lines.extend(wrapped_tools.into_iter().map(Line::from));

            lines.truncate(details_height as usize);
            lines
        }
    }
}

pub(crate) fn mcp_details_height_for_inner_height(inner_height: u16) -> u16 {
    match inner_height {
        0..=4 => 0,
        5 => 1,
        6..=7 => 2,
        8..=9 => 3,
        10..=11 => 4,
        12..=13 => 5,
        _ => 6,
    }
}

pub(crate) fn mcp_panel_controls_line() -> &'static str {
    "Session-only toggles | Enter/Space toggle | Esc close"
}

pub(super) fn mcp_table_model(state: &AppState, table_height: u16) -> McpTableModel {
    let columns = vec![
        "Name".to_string(),
        "Visible tools".to_string(),
        "Status".to_string(),
    ];
    let all_rows = state
        .mcp_servers
        .iter()
        .map(|server| {
            let visible_tools = state
                .mcp_visible_tool_count_for_server_name(server.name.as_str())
                .to_string();
            [
                server.name.clone(),
                visible_tools,
                server.state.icon().to_string(),
            ]
        })
        .collect::<Vec<_>>();

    let selected_global = if all_rows.is_empty() {
        None
    } else {
        Some(
            state
                .mcp_panel_selection
                .min(all_rows.len().saturating_sub(1)),
        )
    };

    let table_view_rows = table_height.saturating_sub(1) as usize;
    let visible_body_rows = table_view_rows.saturating_sub(1);
    let (window_start, window_len) = if visible_body_rows == 0 || all_rows.is_empty() {
        (0, 0)
    } else {
        let max_start = all_rows.len().saturating_sub(visible_body_rows);
        let start = selected_global
            .unwrap_or(0)
            .saturating_sub(visible_body_rows.saturating_sub(1))
            .min(max_start);
        (start, visible_body_rows)
    };

    let rows = all_rows
        .iter()
        .skip(window_start)
        .take(window_len)
        .cloned()
        .collect::<Vec<_>>();

    let selected = selected_global.and_then(|global| {
        if global >= window_start && global < window_start.saturating_add(rows.len()) {
            Some(global.saturating_sub(window_start))
        } else {
            None
        }
    });

    let overflow_cue = if all_rows.len() > window_len && window_len > 0 {
        let start = window_start.saturating_add(1);
        let end = window_start.saturating_add(window_len).min(all_rows.len());
        Some(format!(
            "↑/↓ or j/k | Enter/Space toggle | Esc close | {}-{} / {}",
            start,
            end,
            all_rows.len()
        ))
    } else {
        None
    };

    McpTableModel {
        columns,
        rows,
        selected,
        overflow_cue,
    }
}

pub(super) fn command_palette_table_model(
    state: &AppState,
    popup_width: u16,
    popup_height: u16,
) -> CommandPaletteTableModel {
    let actions = state.command_palette_actions();
    let _ = popup_width;
    let columns = vec!["Action".to_string(), "Summary".to_string()];

    let all_rows = actions
        .iter()
        .map(|action| {
            [
                action.label().to_string(),
                action.summary().to_string(),
                String::new(),
            ]
        })
        .collect::<Vec<_>>();

    let selected_global = if all_rows.is_empty() {
        None
    } else {
        Some(
            state
                .command_palette_selection
                .min(all_rows.len().saturating_sub(1)),
        )
    };

    let table_view_rows = popup_height.saturating_sub(3) as usize;
    let visible_body_rows = table_view_rows.saturating_sub(1);
    let (window_start, window_len) = if visible_body_rows == 0 || all_rows.is_empty() {
        (0, 0)
    } else {
        let max_start = all_rows.len().saturating_sub(visible_body_rows);
        let start = selected_global
            .unwrap_or(0)
            .saturating_sub(visible_body_rows.saturating_sub(1))
            .min(max_start);
        (start, visible_body_rows)
    };

    let rows = all_rows
        .iter()
        .skip(window_start)
        .take(window_len)
        .cloned()
        .collect::<Vec<_>>();

    let selected = selected_global.and_then(|global| {
        if global >= window_start && global < window_start.saturating_add(rows.len()) {
            Some(global.saturating_sub(window_start))
        } else {
            None
        }
    });

    let overflow_cue = if all_rows.len() > window_len && window_len > 0 {
        let start = window_start.saturating_add(1);
        let end = window_start.saturating_add(window_len).min(all_rows.len());
        Some(format!(
            "Esc close | {}-{} / {}",
            start,
            end,
            all_rows.len()
        ))
    } else {
        None
    };

    CommandPaletteTableModel {
        query_line: format!("Query: {}", state.command_palette_query),
        columns,
        rows,
        selected,
        overflow_cue,
    }
}
