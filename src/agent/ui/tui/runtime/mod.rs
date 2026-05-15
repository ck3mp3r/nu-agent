use std::{
    io::Write,
    path::PathBuf,
    time::{Duration, Instant},
};

use ratatui::symbols;
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    layout::{Margin, Position, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap},
};
mod render_frame;
mod status;
mod terminal_events;
mod terminal_io;
mod tool_hydration;
mod transcript_rows;
mod transcript_window;

#[cfg(test)]
mod test;

#[cfg(test)]
mod hybrid_events_test;

#[cfg(test)]
mod transcript_rows_test;

use crate::agent::protocol::contracts::{SharedUiAction, UiMessageSnapshot};
use crate::agent::protocol::event::UiEvent;
use crate::agent::protocol::event::{PermissionDecisionSubmission, ToolDisplay};
use crate::agent::protocol::skills::DiscoverableSkill as ProtocolDiscoverableSkill;
use crate::agent::protocol::slash::{slash_command_label, slash_command_summary};
#[cfg(test)]
use crate::agent::ui::tui::state::PromptStatus;
use crate::agent::ui::{
    renderer::UiRenderer,
    tui::{
        interaction::{
            cancel::CancelController,
            dispatch::dispatch_terminal_event,
            input::{TerminalEvent, TerminalKey},
            reducer::{ReducerInput, reduce_with_cancel_controller},
        },
        platform::{
            safety::{RestoreRunError, run_with_restore},
            terminal::{TerminalBackend, TerminalLifecycle, TerminalLifecycleError},
            transport::TuiTransport,
        },
        rendering::{
            layout::{
                LayoutInput, LayoutOutput, input_cursor_row_col, input_pane_height_for_content,
                recompute_layout, wrapped_input_rows,
            },
            theme::TuiTheme,
        },
        state::{
            AppState, CommandPaletteAction, InfoPanel, McpServerState, McpServerUsabilityState,
            McpToggleRequest, ModelPickerOption, TranscriptLineStatus, TranscriptRole,
        },
    },
};
use crate::tools::mcp::runtime::McpServerLifecycle;

fn permission_prompt_summary_line(
    prompt: &crate::agent::ui::tui::state::PermissionPrompt,
) -> String {
    let mode_suffix = prompt
        .mode
        .as_deref()
        .map(|mode| format!(" · mode={mode}"))
        .unwrap_or_default();
    format!(
        "Permission required · {} · {}{mode_suffix}",
        prompt.tool, prompt.source
    )
}

fn append_pre_authorize_display_lines(
    lines: &mut Vec<crate::agent::ui::tui::state::TranscriptLine>,
    display: &ToolDisplay,
) {
    lines.push(crate::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::ToolDisplay,
        text: display.title.clone(),
        rendered: None,
    });

    for section in &display.sections {
        lines.push(crate::agent::ui::tui::state::TranscriptLine {
            role: TranscriptRole::ToolDisplay,
            text: format!("{} ({})", section.label, section.language),
            rendered: None,
        });

        let section_content = if section.language == "diff" {
            enrich_permission_diff_preview(&section.content)
        } else {
            section.content.clone()
        };

        let markdown = format!("```{}\n{}\n```", section.language, section_content);
        for rendered_line in crate::agent::ui::tui::markdown::project_markdown_to_lines(&markdown) {
            let text = crate::agent::ui::tui::markdown::rendered_line_to_plain_text(&rendered_line);
            if text.trim().is_empty() {
                continue;
            }
            lines.push(crate::agent::ui::tui::state::TranscriptLine {
                role: TranscriptRole::ToolDisplay,
                text,
                rendered: Some(rendered_line),
            });
        }
    }
}

fn enrich_permission_diff_preview(diff: &str) -> String {
    let mut old_line: Option<usize> = None;
    let mut new_line: Option<usize> = None;
    let mut out = String::new();

    for segment in diff.split_inclusive('\n') {
        let (line, newline) = if let Some(stripped) = segment.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (segment, "")
        };

        if line.starts_with("@@") {
            old_line = parse_hunk_start(line, '-');
            new_line = parse_hunk_start(line, '+');
            out.push_str(line);
            out.push_str(newline);
            continue;
        }

        if line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("\\ ") {
            out.push_str(line);
            out.push_str(newline);
            continue;
        }

        let mut chars = line.chars();
        let prefix = chars.next();
        let body = chars.as_str();

        match (prefix, old_line, new_line) {
            (Some(' '), Some(old), Some(new)) => {
                out.push_str(&format!(" {:>4} {:>4} │{}{}", old, new, body, newline));
                old_line = Some(old.saturating_add(1));
                new_line = Some(new.saturating_add(1));
            }
            (Some('-'), Some(old), _) => {
                out.push_str(&format!("-{:>4}      │{}{}", old, body, newline));
                old_line = Some(old.saturating_add(1));
            }
            (Some('+'), _, Some(new)) => {
                out.push_str(&format!("+     {:>4} │{}{}", new, body, newline));
                new_line = Some(new.saturating_add(1));
            }
            _ => {
                out.push_str(line);
                out.push_str(newline);
            }
        }
    }

    out
}

fn parse_hunk_start(line: &str, prefix: char) -> Option<usize> {
    let mut chars = line.chars();
    while let Some(ch) = chars.next() {
        if ch == prefix {
            let remainder = chars.as_str();
            let digits: String = remainder
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect();
            if digits.is_empty() {
                return None;
            }
            return digits.parse::<usize>().ok();
        }
    }
    None
}

fn permission_prompt_transcript_lines(
    prompt: &crate::agent::ui::tui::state::PermissionPrompt,
) -> Vec<crate::agent::ui::tui::state::TranscriptLine> {
    let mut lines = Vec::new();

    if let Some(display) = &prompt.pre_authorize_display {
        append_pre_authorize_display_lines(&mut lines, display);
    }

    lines.push(crate::agent::ui::tui::state::TranscriptLine {
        role: TranscriptRole::System,
        text: permission_prompt_summary_line(prompt),
        rendered: None,
    });

    if prompt.tool.contains("__") {
        lines.push(crate::agent::ui::tui::state::TranscriptLine {
            role: TranscriptRole::System,
            text: prompt.summary.clone(),
            rendered: None,
        });
    }

    lines
}

fn permission_prompt_controls_text() -> String {
    "a allow_once · A allow_always (session) · d/Esc deny".to_string()
}

fn transcript_pane_regions(
    transcript_area: Rect,
    reserve_footer_row: bool,
) -> (Rect, Option<Rect>) {
    if transcript_area.height == 0 {
        return (transcript_area, None);
    }

    if !reserve_footer_row {
        return (transcript_area, None);
    }

    let footer = Rect {
        x: transcript_area.x,
        y: transcript_area
            .y
            .saturating_add(transcript_area.height.saturating_sub(1)),
        width: transcript_area.width,
        height: 1,
    };
    let content = Rect {
        x: transcript_area.x,
        y: transcript_area.y,
        width: transcript_area.width,
        height: transcript_area.height.saturating_sub(1),
    };

    (content, Some(footer))
}

fn permission_prompt_footer_line(theme: &TuiTheme) -> Line<'static> {
    Line::from(vec![Span::styled(
        permission_prompt_controls_text(),
        theme.role_system,
    )])
}

fn permission_prompt_footer_line_for_state(
    state: &AppState,
    theme: &TuiTheme,
) -> Option<Line<'static>> {
    state
        .permission_prompt
        .as_ref()
        .map(|_| permission_prompt_footer_line(theme))
}

#[cfg(test)]
fn permission_prompt_footer_text_for_state(state: &AppState) -> Option<String> {
    state
        .permission_prompt
        .as_ref()
        .map(|_| permission_prompt_controls_text())
}

fn transcript_content_visible_lines_for_render(
    transcript_area_height: u16,
    has_prompt_footer: bool,
) -> usize {
    let reserved_footer_rows = usize::from(has_prompt_footer && transcript_area_height > 0);
    transcript_area_height
        .saturating_sub(1) // transcript top border
        .saturating_sub(reserved_footer_rows as u16) as usize
}

fn transcript_row_budget_for_content_area(content_area_height: u16) -> usize {
    content_area_height.saturating_sub(1) as usize
}

fn required_permission_prompt_line_index_for_render(
    state: &AppState,
    transcript_len: usize,
) -> Option<usize> {
    let prompt = state.permission_prompt.as_ref()?;
    let prompt_lines = permission_prompt_transcript_lines(prompt);
    if prompt_lines.is_empty() {
        return None;
    }

    let required_offset = prompt_lines.len().saturating_sub(1);

    let insert_at = prompt
        .attached_tool_transcript_line_index
        .map(|line_index| {
            line_index
                .saturating_add(1)
                .min(state.transcript_preview.len())
        })
        .unwrap_or(state.transcript_preview.len());

    if transcript_len == 0 {
        return None;
    }

    let absolute = insert_at.saturating_add(required_offset);
    Some(absolute.min(transcript_len.saturating_sub(1)))
}

fn required_permission_prompt_line_for_window_selection(
    state: &AppState,
    transcript_len: usize,
) -> Option<usize> {
    if !state.should_auto_recenter_permission_prompt_row() {
        return None;
    }

    required_permission_prompt_line_index_for_render(state, transcript_len)
}

fn transcript_with_permission_prompt_for_render(
    state: &AppState,
) -> Vec<crate::agent::ui::tui::state::TranscriptLine> {
    let mut transcript = state.transcript_preview.clone();
    let Some(prompt) = state.permission_prompt.as_ref() else {
        return transcript;
    };

    let prompt_lines = permission_prompt_transcript_lines(prompt);
    if prompt_lines.is_empty() {
        return transcript;
    }

    let insert_at = prompt
        .attached_tool_transcript_line_index
        .map(|line_index| line_index.saturating_add(1).min(transcript.len()))
        .unwrap_or(transcript.len());
    transcript.splice(insert_at..insert_at, prompt_lines);
    transcript
}

fn transcript_line_statuses_for_render(
    state: &AppState,
    transcript_len: usize,
) -> Vec<Option<TranscriptLineStatus>> {
    let Some(prompt) = state.permission_prompt.as_ref() else {
        return (0..transcript_len)
            .map(|idx| state.transcript_line_status_for_index(idx))
            .collect();
    };

    let prompt_lines_len = permission_prompt_transcript_lines(prompt).len();
    let insert_at = prompt
        .attached_tool_transcript_line_index
        .map(|line_index| {
            line_index
                .saturating_add(1)
                .min(state.transcript_preview.len())
        })
        .unwrap_or(state.transcript_preview.len());

    (0..transcript_len)
        .map(|render_idx| {
            if render_idx < insert_at {
                return state.transcript_line_status_for_index(render_idx);
            }

            if render_idx < insert_at.saturating_add(prompt_lines_len) {
                return None;
            }

            let source_idx = render_idx.saturating_sub(prompt_lines_len);
            state.transcript_line_status_for_index(source_idx)
        })
        .collect()
}

use render_frame::{
    ModalPanelKind, current_time_millis, modal_rect_for_panel, vertical_heights_for_main_with_input,
};
#[cfg(test)]
use status::visual_indicator_line;
use status::{
    availability_label, build_status_lines, compact_status_line, cursor_style_for_mode,
    lane_2_status_line, transcript_selection_range_for_render, transcript_title_for_render,
};
#[cfg(test)]
pub use terminal_events::ScriptedTerminalEvents;
#[cfg(test)]
pub(crate) use terminal_events::map_crossterm_event_for_test;
#[allow(unused_imports)]
pub use terminal_events::{
    CrosstermTerminalEvents, HybridTerminalEvents, InputSourceDiagnostics, TerminalEventSource,
};
pub use terminal_io::{TtyTerminalEvents, open_tty_reader};
use tool_hydration::{extract_tool_name, parse_persisted_tool_status_line};
use transcript_rows::render_transcript_lines;
#[cfg(test)]
use transcript_rows::{
    build_row_spans, indicator_style_for_status, lane_prefix_spans, prompt_indicator_for_status,
};
use transcript_window::{
    should_insert_transition_spacer, visible_transcript_window_for_render_with_required_line,
};

fn wrapped_visual_rows_for_rendered_line(rendered_line: &Line<'_>, content_width: usize) -> usize {
    let width = rendered_line
        .spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum::<usize>()
        .max(1);
    width.div_ceil(content_width.max(1))
}

fn input_prompt_prefix(mode: crate::agent::ui::tui::state::InputMode) -> &'static str {
    match mode {
        crate::agent::ui::tui::state::InputMode::Insert => "❯ ",
        crate::agent::ui::tui::state::InputMode::Normal
        | crate::agent::ui::tui::state::InputMode::Visual => "❮ ",
    }
}

fn help_panel_total_visual_rows(lines: &[Line<'_>], content_width: usize) -> usize {
    lines
        .iter()
        .map(|line| wrapped_visual_rows_for_rendered_line(line, content_width.max(1)))
        .sum()
}

fn help_panel_max_scroll(lines: &[Line<'_>], viewport_height: u16, content_width: u16) -> usize {
    let visible_rows = viewport_height.max(1) as usize;
    let total_rows = help_panel_total_visual_rows(lines, content_width.max(1) as usize);
    total_rows.saturating_sub(visible_rows)
}

fn help_panel_overflow_cue(
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

fn command_palette_action_summary(action: CommandPaletteAction) -> &'static str {
    match action {
        CommandPaletteAction::Compact => "Run /compact now",
        CommandPaletteAction::Help => "View key help",
        CommandPaletteAction::Status => "View runtime status",
        CommandPaletteAction::Mcps => "Manage MCP servers",
        CommandPaletteAction::Skills => "List available skills",
        CommandPaletteAction::Models => "Open model picker",
    }
}

fn command_palette_action_keys() -> &'static str {
    "↑/↓ or Ctrl-N · Enter · Esc"
}

fn command_palette_title(overflow_cue: Option<&str>) -> String {
    let base = "Command Palette";
    let global_hint = command_palette_action_keys();
    if let Some(cue) = overflow_cue {
        format!("{base} ({global_hint} | {cue})")
    } else {
        format!("{base} ({global_hint})")
    }
}

fn command_palette_action_label(action: CommandPaletteAction) -> &'static str {
    match action {
        CommandPaletteAction::Compact => "/compact",
        CommandPaletteAction::Help => "Help",
        CommandPaletteAction::Status => "Status",
        CommandPaletteAction::Mcps => "MCPs",
        CommandPaletteAction::Skills => "Skills",
        CommandPaletteAction::Models => "Models",
    }
}

fn inline_slash_lines_for_render(state: &AppState) -> Vec<Line<'static>> {
    if !state.inline_slash_open {
        return Vec::new();
    }

    state
        .inline_slash_suggestions()
        .iter()
        .enumerate()
        .map(|(idx, command)| {
            let marker = if idx == state.inline_slash_selection {
                "❯"
            } else {
                " "
            };
            let label = slash_command_label(*command);
            let summary = slash_command_summary(*command);
            Line::from(format!("{marker} {label} — {summary}"))
        })
        .collect()
}

fn input_buffer_for_layout(state: &AppState) -> String {
    if !state.inline_slash_open {
        return state.input.buffer.clone();
    }

    let mut synthetic = state.input.buffer.clone();
    for _ in state.inline_slash_suggestions() {
        synthetic.push('\n');
    }
    synthetic
}

const MODEL_PICKER_EMPTY_STATE_MESSAGE: &str = "No models available in cached startup config.";

fn skills_panel_lines(state: &AppState) -> (&'static str, Vec<Line<'static>>) {
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
struct CommandPaletteTableModel {
    query_line: String,
    columns: Vec<String>,
    rows: Vec<[String; 3]>,
    selected: Option<usize>,
    overflow_cue: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct McpTableModel {
    columns: Vec<String>,
    rows: Vec<[String; 3]>,
    selected: Option<usize>,
    overflow_cue: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct McpSelectedDetails {
    server_line: String,
    error_line: String,
    tools_line: String,
    tool_names: Vec<String>,
    compact_single_line: String,
}

fn mcp_state_label(state: McpServerUsabilityState) -> &'static str {
    match state {
        McpServerUsabilityState::Enabled => "enabled",
        McpServerUsabilityState::Disabled => "disabled",
        McpServerUsabilityState::Failed => "failed",
    }
}

fn mcp_status_icon(state: McpServerUsabilityState) -> &'static str {
    match state {
        McpServerUsabilityState::Enabled => "🟢",
        McpServerUsabilityState::Disabled => "⚪",
        McpServerUsabilityState::Failed => "🔴",
    }
}

const MCP_STATUS_COLUMN_WIDTH: u16 = 6;

fn mcp_selected_details(state: &AppState) -> Option<McpSelectedDetails> {
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
    let server_line = format!(
        "Server: {} ({})",
        server.name,
        mcp_state_label(server.state)
    );
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

fn mcp_tool_lines_wrapped(tool_names: &[String], max_lines: usize, width: usize) -> Vec<String> {
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

fn mcp_selected_details_lines(
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

fn mcp_details_height_for_inner_height(inner_height: u16) -> u16 {
    if inner_height >= 14 {
        6
    } else if inner_height >= 12 {
        5
    } else if inner_height >= 10 {
        4
    } else if inner_height >= 8 {
        3
    } else if inner_height >= 6 {
        2
    } else if inner_height >= 5 {
        1
    } else {
        0
    }
}

fn mcp_panel_controls_line() -> &'static str {
    "Session-only toggles | Enter/Space toggle | Esc close"
}

fn mcp_table_model(state: &AppState, table_height: u16) -> McpTableModel {
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
                mcp_status_icon(server.state).to_string(),
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

fn command_palette_table_model(
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
                command_palette_action_label(*action).to_string(),
                command_palette_action_summary(*action).to_string(),
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

#[cfg(test)]
fn help_panel_visible_window(
    lines: &[Line<'_>],
    content_width: usize,
    scroll: usize,
    rows: usize,
) -> Vec<Line<'static>> {
    let mut visual_rows = Vec::new();
    let width = content_width.max(1);

    for line in lines {
        let text = line.to_string();
        if text.is_empty() {
            visual_rows.push(String::new());
            continue;
        }
        let chars = text.chars().collect::<Vec<_>>();
        for chunk in chars.chunks(width) {
            visual_rows.push(chunk.iter().collect::<String>());
        }
    }

    visual_rows
        .into_iter()
        .skip(scroll)
        .take(rows)
        .map(Line::from)
        .collect()
}

#[derive(Debug)]
pub struct RuntimeCoordinator {
    state: AppState,
    transport: TuiTransport,
    cancel_controller: CancelController,
    layout: LayoutOutput,
    terminal_columns: u16,
    terminal_rows: u16,
    side_pane_visible: Option<bool>,
    quit_requested: bool,
    fatal_error: Option<String>,
    active_model_identity: String,
    input_backend_status: String,
    last_input_poll_status: String,
    last_input_error: Option<String>,
    input_watchdog_started_at: Instant,
    input_watchdog_timeout: Duration,
    repo_branch_tracker: Option<status::RepoBranchTracker>,
    theme: TuiTheme,
}

impl RuntimeCoordinator {
    const DEFAULT_INPUT_WATCHDOG_TIMEOUT: Duration = Duration::from_secs(5);

    pub fn new(columns: u16, rows: u16, side_pane_visible: Option<bool>) -> Self {
        Self::new_with_watchdog(
            columns,
            rows,
            side_pane_visible,
            Self::DEFAULT_INPUT_WATCHDOG_TIMEOUT,
        )
    }

    pub(crate) fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
    ) {
        for message in messages {
            if let Some(usage) = message.usage() {
                self.state.hydrate_usage(
                    usage.input_tokens(),
                    usage.output_tokens(),
                    usage.total_tokens(),
                );
            }
            let role = match message.role() {
                "user" => TranscriptRole::User,
                "assistant" => TranscriptRole::Assistant,
                "tool" => TranscriptRole::Tool,
                _ => TranscriptRole::System,
            };
            let message_content = message.content();
            if message_content.trim().is_empty() {
                continue;
            }
            if role == TranscriptRole::Assistant {
                for line in self.state.project_assistant_markdown_lines(message_content) {
                    let plain_text =
                        crate::agent::ui::tui::markdown::rendered_line_to_plain_text(&line);
                    if !plain_text.trim().is_empty() {
                        self.state.push_transcript_rendered_line(role, line);
                    }
                }
                continue;
            }

            if role == TranscriptRole::Tool {
                let persisted = message_content.trim();
                if let Some(arguments) = message.tool_arguments() {
                    let success = message.tool_success().unwrap_or(true);
                    self.state
                        .start_tool_call(extract_tool_name(persisted), arguments);
                    self.state
                        .finish_tool_call(extract_tool_name(persisted), arguments, success);
                    continue;
                }
                if let Some((name, arguments, success)) =
                    parse_persisted_tool_status_line(persisted)
                {
                    self.state.start_tool_call(name, arguments);
                    self.state.finish_tool_call(name, arguments, success);
                    continue;
                }
            }

            for line in message_content.lines() {
                if !line.trim().is_empty() {
                    self.state.push_transcript_line(role, line.to_string());
                }
            }
        }
    }

    fn new_with_watchdog(
        columns: u16,
        rows: u16,
        _side_pane_visible: Option<bool>,
        input_watchdog_timeout: Duration,
    ) -> Self {
        let side_pane_visible = Some(false);
        let layout = recompute_layout(LayoutInput {
            columns,
            rows,
            side_pane_visible,
            input_height: None,
        });
        let mut coordinator = Self {
            state: AppState::new(),
            transport: TuiTransport::new(),
            cancel_controller: CancelController::new(),
            layout,
            terminal_columns: columns,
            terminal_rows: rows,
            side_pane_visible,
            quit_requested: false,
            fatal_error: None,
            active_model_identity: "unknown".to_string(),
            input_backend_status: "unknown".to_string(),
            last_input_poll_status: "waiting for input poll".to_string(),
            last_input_error: None,
            input_watchdog_started_at: Instant::now(),
            input_watchdog_timeout,
            repo_branch_tracker: None,
            theme: TuiTheme::default(),
        };
        coordinator.sync_transcript_viewport_lines_with_layout();
        coordinator
    }

    #[cfg(test)]
    pub fn new_for_test_with_watchdog(
        columns: u16,
        rows: u16,
        side_pane_visible: Option<bool>,
        input_watchdog_timeout: Duration,
    ) -> Self {
        Self::new_with_watchdog(columns, rows, side_pane_visible, input_watchdog_timeout)
    }

    #[cfg(test)]
    pub fn state(&self) -> &AppState {
        &self.state
    }

    #[cfg(test)]
    pub fn layout(&self) -> LayoutOutput {
        self.layout
    }

    #[cfg(test)]
    pub fn cancel_controller(&self) -> &CancelController {
        &self.cancel_controller
    }

    #[cfg(test)]
    pub fn input_diagnostics_snapshot(&self) -> (String, String, Option<String>) {
        (
            self.input_backend_status.clone(),
            self.last_input_poll_status.clone(),
            self.last_input_error.clone(),
        )
    }

    pub(crate) fn take_submitted_prompt(&mut self) -> Option<String> {
        self.state.take_next_prompt_for_execution()
    }

    pub(crate) fn take_next_model_picker_launch_request(&mut self) -> bool {
        self.state.take_next_model_picker_launch_request()
    }

    pub(crate) fn set_active_model_identity(&mut self, active_model_identity: String) {
        self.active_model_identity = active_model_identity;
    }

    pub(crate) fn set_mcp_lifecycle_projection(&mut self, projection: Vec<McpServerLifecycle>) {
        let servers = projection
            .into_iter()
            .map(|server| {
                let name = server.name;
                self.state.set_mcp_visible_tool_count_by_server_name(
                    name.as_str(),
                    server.visible_tool_count,
                );
                McpServerState {
                    name,
                    state: match (server.enabled, server.connected) {
                        (true, true) => McpServerUsabilityState::Enabled,
                        (true, false) => McpServerUsabilityState::Failed,
                        (false, _) => McpServerUsabilityState::Disabled,
                    },
                }
            })
            .collect();
        self.state.set_mcp_servers(servers);
    }

    pub(crate) fn set_skills_projection(&mut self, skills: Vec<ProtocolDiscoverableSkill>) {
        let mapped = skills
            .into_iter()
            .map(|skill| crate::agent::ui::tui::state::DiscoverableSkill {
                source_priority: skill.source.priority(),
                source: skill.source.label().to_string(),
                name: skill.name,
            })
            .collect();
        self.state.set_discoverable_skills(mapped);
    }

    pub(crate) fn mark_skills_discovery_failed(&mut self) {
        self.state.mark_skills_discovery_failed();
    }

    pub(crate) fn set_llm_visible_mcp_tool_count(&mut self, count: usize) {
        self.state.set_llm_visible_mcp_tool_count(count);
    }

    pub(crate) fn set_mcp_visible_tool_count_by_server_name(
        &mut self,
        server_name: &str,
        count: usize,
    ) {
        self.state
            .set_mcp_visible_tool_count_by_server_name(server_name, count);
    }

    pub(crate) fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        server_name: &str,
        names: Vec<String>,
    ) {
        self.state
            .set_mcp_visible_tool_names_by_server_name(server_name, names);
    }

    pub(crate) fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.state.set_context_window_max_tokens(max_tokens);
    }

    pub(crate) fn set_model_picker_options(&mut self, options: Vec<ModelPickerOption>) {
        self.state.set_model_picker_options(options);
    }

    pub(crate) fn set_repo_branch_caller_cwd(&mut self, caller_cwd: Option<PathBuf>) {
        self.repo_branch_tracker = Some(status::RepoBranchTracker::from_caller_cwd(caller_cwd));
    }

    pub(crate) fn take_next_mcp_toggle_request(&mut self) -> Option<McpToggleRequest> {
        self.state.take_next_mcp_toggle_request()
    }

    pub(crate) fn take_next_model_switch_request(&mut self) -> Option<String> {
        self.state.take_next_model_switch_request()
    }

    pub(crate) fn take_next_permission_decision_submission(
        &mut self,
    ) -> Option<PermissionDecisionSubmission> {
        self.state.take_next_permission_decision_submission()
    }

    pub(crate) fn set_mcp_server_state(
        &mut self,
        server_name: &str,
        state: McpServerUsabilityState,
    ) -> bool {
        self.state.set_mcp_server_state_by_name(server_name, state)
    }

    pub(crate) fn set_mcp_server_state_with_details(
        &mut self,
        server_name: &str,
        state: McpServerUsabilityState,
        reason: Option<String>,
        llm_visible_mcp_tool_count: usize,
    ) -> bool {
        self.state
            .set_llm_visible_mcp_tool_count(llm_visible_mcp_tool_count);
        self.state
            .set_mcp_server_state_by_name_with_reason(server_name, state, reason)
    }

    pub fn quit_requested(&self) -> bool {
        self.quit_requested
    }

    pub(crate) fn fatal_error(&self) -> Option<&str> {
        self.fatal_error.as_deref()
    }

    pub fn take_cancel_requested(&self) -> bool {
        self.cancel_controller.take_cancel_requested()
    }

    pub fn enqueue_ui_event(&mut self, event: UiEvent) {
        self.transport.enqueue_ui_event(event);
    }

    pub fn poll_terminal_event(&mut self, event_source: &mut impl TerminalEventSource) {
        if let Some(tracker) = self.repo_branch_tracker.as_mut() {
            tracker.tick();
        }

        let poll_result = event_source.poll_event();
        let diagnostics = event_source.diagnostics();
        self.update_input_diagnostics(&diagnostics);

        let event = match poll_result {
            Ok(Some(event)) => event,
            Ok(None) => {
                self.maybe_trigger_input_watchdog(&diagnostics);
                return;
            }
            Err(error) => {
                if Self::both_backends_unavailable(&diagnostics) {
                    self.trigger_no_interactive_backend_fail_fast(Some(error));
                    return;
                }
                self.state.status_line = format!("Terminal input error: {error}");
                self.fatal_error = Some(self.state.status_line.clone());
                self.quit_requested = true;
                self.cancel_controller.request_cancel();
                return;
            }
        };

        self.last_input_poll_status = format!("event from {}", diagnostics.active_backend);

        if let TerminalEvent::Key(TerminalKey::Esc) = event
            && self.state.phase == crate::agent::ui::tui::state::UiPhase::Idle
            && !self.state.command_palette_open
            && self.state.info_panel.is_none()
        {
            self.state.status_line = "Esc pressed. Press Ctrl+C to quit.".to_string();
        }

        if let TerminalEvent::Key(TerminalKey::CtrlC) = event {
            self.quit_requested = true;
            self.cancel_controller.request_cancel();
        }

        if let TerminalEvent::Resize(resize) = event {
            self.terminal_columns = resize.columns;
            self.terminal_rows = resize.rows;
            let input_height = input_pane_height_for_content(
                &input_buffer_for_layout(&self.state),
                resize.columns,
            );
            self.layout = recompute_layout(LayoutInput {
                columns: resize.columns,
                rows: resize.rows,
                side_pane_visible: self.side_pane_visible,
                input_height: Some(input_height),
            });
        }

        let _ = dispatch_terminal_event(&mut self.state, &event, Some(&self.cancel_controller));
        self.recompute_layout_for_current_input();
        self.flush_clipboard_request();
        self.quit_requested |= self.state.quit_requested;

        self.sync_transcript_viewport_lines_with_layout();
    }

    fn sync_transcript_viewport_lines_with_layout(&mut self) {
        let visible_lines = self.layout.transcript.height.saturating_sub(1) as usize;
        self.state
            .set_transcript_viewport_lines(visible_lines.max(1));
    }

    fn execute_shared_ui_action(&mut self, action: SharedUiAction) -> bool {
        match action {
            SharedUiAction::Help => {
                self.state.open_info_panel(InfoPanel::Help);
                true
            }
            SharedUiAction::Status => {
                self.state.open_info_panel(InfoPanel::Status);
                true
            }
            SharedUiAction::Mcps => {
                self.state.open_info_panel(InfoPanel::Mcps);
                true
            }
            SharedUiAction::Models => {
                self.state.open_model_picker();
                true
            }
        }
    }

    fn recompute_layout_for_current_input(&mut self) {
        let input_height = input_pane_height_for_content(
            &input_buffer_for_layout(&self.state),
            self.layout.transcript.width,
        );
        self.layout = recompute_layout(LayoutInput {
            columns: self.terminal_columns,
            rows: self.terminal_rows,
            side_pane_visible: self.side_pane_visible,
            input_height: Some(input_height),
        });
    }

    fn flush_clipboard_request(&mut self) {
        let Some(text) = self.state.take_clipboard_request() else {
            return;
        };

        match arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)) {
            Ok(()) => {
                self.state.status_line = "Copied selection to clipboard.".to_string();
            }
            Err(error) => {
                self.state.status_line = format!("Clipboard copy failed: {error}");
            }
        }
    }

    fn update_input_diagnostics(&mut self, diagnostics: &InputSourceDiagnostics) {
        let primary = availability_label(diagnostics.primary_available);
        let fallback = availability_label(diagnostics.fallback_available);
        self.input_backend_status = format!(
            "active={}, crossterm={}, /dev/tty={}",
            diagnostics.active_backend, primary, fallback
        );
        self.last_input_poll_status = diagnostics.last_poll_state.clone();
        self.last_input_error = diagnostics.last_error.clone();
    }

    fn maybe_trigger_input_watchdog(&mut self, diagnostics: &InputSourceDiagnostics) {
        if self.quit_requested || self.fatal_error.is_some() {
            return;
        }

        if !Self::both_backends_unavailable(diagnostics) {
            return;
        }

        if self.input_watchdog_started_at.elapsed() < self.input_watchdog_timeout {
            return;
        }

        self.trigger_no_interactive_backend_fail_fast(None);
    }

    fn both_backends_unavailable(diagnostics: &InputSourceDiagnostics) -> bool {
        diagnostics.primary_available == Some(false)
            && diagnostics.fallback_available == Some(false)
    }

    fn trigger_no_interactive_backend_fail_fast(&mut self, poll_error: Option<String>) {
        if let Some(error) = poll_error
            && self.last_input_error.is_none()
        {
            self.last_input_error = Some(error);
        }

        let mut message = format!(
            "No interactive input backend available (crossterm and /dev/tty unavailable). Last poll: {}.",
            self.last_input_poll_status
        );
        if let Some(error) = self.last_input_error.as_deref() {
            message.push_str(&format!(" Last error: {error}."));
        }
        message.push_str(" Run `agent` in an interactive terminal and verify TTY access.");

        self.state.status_line = message.clone();
        self.fatal_error = Some(message);
        self.quit_requested = true;
        self.cancel_controller.request_cancel();
    }

    pub fn drain_transport(&mut self) {
        while let Some(item) = self.transport.poll_next() {
            reduce_with_cancel_controller(
                &mut self.state,
                ReducerInput::from(item),
                Some(&self.cancel_controller),
            );
        }
    }

    fn render_frame(&self, live: &mut Option<LiveTerminalUi>) -> Result<(), String> {
        let Some(live) = live.as_mut() else {
            return Ok(());
        };

        live.terminal
            .draw(|frame| {
                let area = frame.area();
                let has_side = self.layout.side_pane.is_some();
                let horizontal = if has_side {
                    Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                        .split(area)
                } else {
                    Layout::default()
                        .direction(Direction::Horizontal)
                        .constraints([Constraint::Percentage(100)])
                        .split(area)
                };

                let main = horizontal[0];
                let side_margin = if main.width >= 8 { 2 } else { 0 };
                let content_main = main.inner(Margin {
                    vertical: 0,
                    horizontal: side_margin,
                });
                let (header_h, transcript_h, status_h, input_h) =
                    vertical_heights_for_main_with_input(
                        content_main.height,
                        self.layout.input.height,
                    );
                let vertical = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(header_h),
                        Constraint::Length(transcript_h),
                        Constraint::Length(input_h),
                        Constraint::Length(status_h),
                    ])
                    .split(content_main);

                let transcript_for_render =
                    transcript_with_permission_prompt_for_render(&self.state);
                let transcript_line_statuses =
                    transcript_line_statuses_for_render(&self.state, transcript_for_render.len());
                let required_prompt_line = required_permission_prompt_line_for_window_selection(
                    &self.state,
                    transcript_for_render.len(),
                );
                let has_permission_footer = self.state.permission_prompt.is_some();
                let visible_transcript_lines = transcript_content_visible_lines_for_render(
                    vertical[1].height,
                    has_permission_footer,
                );
                let (window_start, window_lines) =
                    visible_transcript_window_for_render_with_required_line(
                        &transcript_for_render,
                        visible_transcript_lines,
                        self.state.transcript_scroll_lines_from_bottom,
                        self.state.transcript_follow_tail,
                        vertical[1].width as usize,
                        required_prompt_line,
                        &transcript_line_statuses,
                    );
                let selected =
                    transcript_selection_range_for_render(&self.state, transcript_for_render.len());
                let mut transcript = Vec::new();
                let mut prev_role: Option<TranscriptRole> = None;
                let (transcript_content_area, transcript_footer_area) =
                    transcript_pane_regions(vertical[1], has_permission_footer);
                let transcript_row_budget =
                    transcript_row_budget_for_content_area(transcript_content_area.height);
                let mut transcript_rows_used = 0usize;
                for (offset, line) in window_lines.into_iter().enumerate() {
                    let global_idx = window_start.saturating_add(offset);
                    if should_insert_transition_spacer(prev_role, line.role) {
                        if transcript_rows_used >= transcript_row_budget {
                            break;
                        }
                        transcript.push(Line::from(vec![Span::raw(" ")]));
                        transcript_rows_used = transcript_rows_used.saturating_add(1);
                    }
                    let line_status = transcript_line_statuses.get(global_idx).copied().flatten();
                    let is_cursor_line = self.state.transcript_cursor_index() == Some(global_idx)
                        && self.state.input_mode != crate::agent::ui::tui::state::InputMode::Insert;
                    let is_selected = selected
                        .map(|(start, end)| global_idx >= start && global_idx <= end)
                        .unwrap_or(false);
                    let rendered_lines = render_transcript_lines(
                        line,
                        vertical[1].width as usize,
                        is_selected,
                        is_cursor_line,
                        line_status,
                        current_time_millis(),
                        &self.theme,
                    );

                    for rendered_line in rendered_lines {
                        let visual_rows = wrapped_visual_rows_for_rendered_line(
                            &rendered_line,
                            vertical[1].width as usize,
                        );
                        if transcript_rows_used.saturating_add(visual_rows) > transcript_row_budget
                        {
                            if transcript_rows_used == 0 {
                                transcript.push(rendered_line);
                                transcript_rows_used = transcript_row_budget;
                            }
                            break;
                        }
                        transcript.push(rendered_line);
                        transcript_rows_used = transcript_rows_used.saturating_add(visual_rows);
                    }
                    prev_role = transcript_for_render
                        .get(global_idx)
                        .map(|entry| entry.role);

                    if transcript_rows_used >= transcript_row_budget {
                        break;
                    }
                }
                let transcript_view_height = transcript_row_budget;
                let _transcript_title =
                    transcript_title_for_render(&self.state, transcript_for_render.len());
                let transcript_border_style = if self.state.pane_focus
                    == crate::agent::ui::tui::state::PaneFocus::Transcript
                {
                    self.theme.focus
                } else {
                    Style::default()
                };
                let transcript_widget = if transcript_view_height == 0 {
                    Paragraph::new(Text::from(Vec::<Line>::new()))
                        .block(
                            Block::default()
                                .borders(Borders::TOP)
                                .border_style(transcript_border_style),
                        )
                        .wrap(Wrap { trim: false })
                } else {
                    Paragraph::new(Text::from(transcript))
                        .block(
                            Block::default()
                                .borders(Borders::TOP)
                                .border_style(transcript_border_style),
                        )
                        .wrap(Wrap { trim: false })
                };
                if vertical[1].height > 0 {
                    frame.render_widget(Clear, vertical[1]);
                    if transcript_content_area.height > 0 {
                        frame.render_widget(transcript_widget, transcript_content_area);
                    }
                    if let (Some(footer_area), Some(footer_line)) = (
                        transcript_footer_area,
                        permission_prompt_footer_line_for_state(&self.state, &self.theme),
                    ) {
                        let footer_widget = Paragraph::new(Text::from(vec![footer_line]))
                            .wrap(Wrap { trim: false });
                        frame.render_widget(footer_widget, footer_area);
                    }
                }

                let lane_1 = compact_status_line(
                    &self.state,
                    &self.active_model_identity,
                    self.repo_branch_tracker
                        .as_ref()
                        .and_then(|tracker| tracker.branch()),
                    &self.input_backend_status,
                    &self.last_input_poll_status,
                    self.last_input_error.as_deref(),
                    vertical[3].width as usize,
                );
                let lane_2 = lane_2_status_line(&self.state, vertical[3].width as usize);
                let _status_lines = build_status_lines(
                    &self.state,
                    &self.active_model_identity,
                    &self.input_backend_status,
                    &self.last_input_poll_status,
                    self.last_input_error.as_deref(),
                );
                let status_widget =
                    Paragraph::new(Text::from(vec![Line::from(lane_1), Line::from(lane_2)]))
                        .block(Block::default())
                        .wrap(Wrap { trim: false });
                if vertical[3].height > 0 {
                    frame.render_widget(Clear, vertical[3]);
                    frame.render_widget(status_widget, vertical[3]);
                }

                let input_rows = wrapped_input_rows(
                    &self.state.input.buffer,
                    vertical[2].width.saturating_sub(2) as usize,
                );
                let input_border_style =
                    if self.state.pane_focus == crate::agent::ui::tui::state::PaneFocus::Input {
                        self.theme.focus
                    } else {
                        Style::default()
                    };
                let mut input_lines = Vec::new();
                let prompt_prefix = input_prompt_prefix(self.state.input_mode);
                if let Some((first, rest)) = input_rows.split_first() {
                    input_lines.push(Line::from(vec![
                        Span::styled(prompt_prefix, self.theme.input_prompt),
                        Span::raw(first.clone()),
                    ]));
                    for row in rest {
                        input_lines.push(Line::from(vec![Span::raw("  "), Span::raw(row.clone())]));
                    }
                }
                input_lines.extend(inline_slash_lines_for_render(&self.state));
                let input_widget = Paragraph::new(Text::from(input_lines))
                    .block(
                        Block::default()
                            .borders(Borders::TOP)
                            .border_style(input_border_style),
                    )
                    .wrap(Wrap { trim: false });
                if vertical[2].height > 0 {
                    frame.render_widget(Clear, vertical[2]);
                    frame.render_widget(input_widget, vertical[2]);
                }

                if !self.state.input.locked
                    && !self.state.command_palette_open
                    && self.state.info_panel.is_none()
                    && vertical[2].height >= 2
                    && vertical[2].width >= 1
                {
                    let (cursor_row, cursor_col) = input_cursor_row_col(
                        &self.state.input.buffer,
                        self.state.input.cursor,
                        vertical[2].width.saturating_sub(2) as usize,
                    );
                    let x = vertical[2].x.saturating_add(2).saturating_add(cursor_col);
                    let max_x = vertical[2]
                        .x
                        .saturating_add(vertical[2].width.saturating_sub(1));
                    let y = vertical[2]
                        .y
                        .saturating_add(1)
                        .saturating_add(cursor_row)
                        .min(
                            vertical[2]
                                .y
                                .saturating_add(vertical[2].height.saturating_sub(1)),
                        );
                    frame.set_cursor_position(Position { x: x.min(max_x), y });
                }

                if has_side {
                    let side = horizontal[1];
                    let side_widget = Paragraph::new(Line::from("Events pane reserved"))
                        .block(Block::default().borders(Borders::ALL).title("Events"));
                    frame.render_widget(side_widget, side);
                }

                if self.state.command_palette_open {
                    let popup = modal_rect_for_panel(area, ModalPanelKind::CommandPalette);
                    let popup_width = popup.width;
                    let popup_height = popup.height;

                    let model = command_palette_table_model(&self.state, popup_width, popup_height);

                    frame.render_widget(Clear, popup);
                    let outer = Block::default()
                        .borders(Borders::ALL)
                        .border_set(symbols::border::ROUNDED)
                        .title(command_palette_title(model.overflow_cue.as_deref()));
                    frame.render_widget(outer, popup);

                    let inner = popup.inner(Margin {
                        vertical: 1,
                        horizontal: 1,
                    });
                    let rows = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(1), Constraint::Min(0)])
                        .split(inner);

                    frame.render_widget(
                        Paragraph::new(Line::from(model.query_line.clone())),
                        rows[0],
                    );

                    let header = Row::new(vec!["Action", "Summary"]);

                    let table_rows = model.rows.iter().map(|row| {
                        Row::new(vec![Cell::from(row[0].clone()), Cell::from(row[1].clone())])
                    });

                    let widths = vec![Constraint::Length(8), Constraint::Min(12)];

                    let table = Table::new(table_rows, widths)
                        .header(header)
                        .column_spacing(2)
                        .highlight_symbol("❯ ");
                    let mut table_state = TableState::default();
                    table_state.select(model.selected);
                    frame.render_stateful_widget(table, rows[1], &mut table_state);
                }

                if let Some(panel) = self.state.info_panel {
                    let popup = modal_rect_for_panel(
                        area,
                        match panel {
                            InfoPanel::Help => ModalPanelKind::Help,
                            InfoPanel::Status => ModalPanelKind::Status,
                            InfoPanel::Mcps => ModalPanelKind::Mcps,
                            InfoPanel::Skills => ModalPanelKind::Skills,
                        },
                    );

                    match panel {
                        InfoPanel::Mcps => {
                            frame.render_widget(Clear, popup);
                            let inner = popup.inner(Margin {
                                vertical: 1,
                                horizontal: 1,
                            });
                            let details_height = mcp_details_height_for_inner_height(inner.height);

                            let rows = Layout::default()
                                .direction(Direction::Vertical)
                                .constraints([
                                    Constraint::Length(1),
                                    Constraint::Min(1),
                                    Constraint::Length(details_height),
                                ])
                                .split(inner);

                            let model = mcp_table_model(&self.state, rows[1].height);
                            let title = if let Some(cue) = model.overflow_cue.as_deref() {
                                format!("MCPs ({cue})")
                            } else {
                                "MCPs".to_string()
                            };
                            frame.render_widget(
                                Block::default()
                                    .borders(Borders::ALL)
                                    .border_set(symbols::border::ROUNDED)
                                    .title(title),
                                popup,
                            );

                            frame.render_widget(
                                Paragraph::new(Line::from(mcp_panel_controls_line())),
                                rows[0],
                            );

                            let header = Row::new(model.columns.clone());
                            let table_rows = model.rows.iter().map(|row| {
                                Row::new(vec![
                                    Cell::from(row[0].clone()),
                                    Cell::from(row[1].clone()),
                                    Cell::from(row[2].clone()),
                                ])
                            });
                            let widths = [
                                Constraint::Length(18),
                                Constraint::Length(14),
                                Constraint::Length(MCP_STATUS_COLUMN_WIDTH),
                            ];
                            let table = Table::new(table_rows, widths)
                                .header(header)
                                .column_spacing(1)
                                .highlight_symbol("❯ ");
                            let mut table_state = TableState::default();
                            table_state.select(model.selected);
                            frame.render_stateful_widget(table, rows[1], &mut table_state);

                            if details_height > 0 {
                                let details_lines = mcp_selected_details_lines(
                                    &self.state,
                                    details_height,
                                    rows[2].width,
                                );
                                if !details_lines.is_empty() {
                                    let details_widget = Paragraph::new(Text::from(details_lines))
                                        .wrap(Wrap { trim: false });
                                    frame.render_widget(details_widget, rows[2]);
                                }
                            }
                        }
                        _ => {
                            let (title, lines) = match panel {
                                InfoPanel::Help => help_panel_lines(),
                                InfoPanel::Status => status_panel_lines(
                                    &self.state,
                                    &self.active_model_identity,
                                    &self.input_backend_status,
                                    &self.last_input_poll_status,
                                    self.last_input_error.as_deref(),
                                ),
                                InfoPanel::Skills => skills_panel_lines(&self.state),
                                InfoPanel::Mcps => unreachable!("handled above"),
                            };

                            let panel_inner_height = popup.height.saturating_sub(2);
                            let panel_inner_width = popup.width.saturating_sub(2);
                            let panel_scroll =
                                self.state.info_panel_scroll.min(help_panel_max_scroll(
                                    &lines,
                                    panel_inner_height,
                                    panel_inner_width,
                                ));
                            let panel_title = match panel {
                                InfoPanel::Help => {
                                    if let Some(cue) = help_panel_overflow_cue(
                                        &lines,
                                        panel_inner_height,
                                        panel_inner_width,
                                        panel_scroll,
                                    ) {
                                        format!("{title} ({cue})")
                                    } else {
                                        title.to_string()
                                    }
                                }
                                _ => title.to_string(),
                            };

                            frame.render_widget(Clear, popup);
                            frame.render_widget(
                                Paragraph::new(lines)
                                    .block(
                                        Block::default()
                                            .borders(Borders::ALL)
                                            .border_set(symbols::border::ROUNDED)
                                            .title(panel_title),
                                    )
                                    .wrap(Wrap { trim: false })
                                    .scroll((panel_scroll.min(u16::MAX as usize) as u16, 0)),
                                popup,
                            );
                        }
                    }
                }

                if self.state.model_picker_open {
                    let popup = modal_rect_for_panel(area, ModalPanelKind::Models);
                    frame.render_widget(Clear, popup);
                    frame.render_widget(
                        Block::default()
                            .borders(Borders::ALL)
                            .border_set(symbols::border::ROUNDED)
                            .title("Models (↑/↓ or Ctrl-N · Enter · Esc)"),
                        popup,
                    );

                    let inner = popup.inner(Margin {
                        vertical: 1,
                        horizontal: 1,
                    });
                    let rows = Layout::default()
                        .direction(Direction::Vertical)
                        .constraints([Constraint::Length(1), Constraint::Min(0)])
                        .split(inner);
                    frame.render_widget(
                        Paragraph::new(Line::from(format!(
                            "Query: {}",
                            self.state.model_picker_query
                        ))),
                        rows[0],
                    );

                    let options = self.state.model_picker_filtered_options();
                    if options.is_empty() {
                        frame.render_widget(
                            Paragraph::new(Line::from(MODEL_PICKER_EMPTY_STATE_MESSAGE)),
                            rows[1],
                        );
                    } else {
                        let table_rows = options.iter().enumerate().map(|(idx, option)| {
                            let active = if option.active { "*" } else { "" };
                            let marker = if idx == self.state.model_picker_selection {
                                "❯ "
                            } else {
                                "  "
                            };
                            Row::new(vec![
                                Cell::from(format!("{marker}{}", option.identity)),
                                Cell::from(active.to_string()),
                            ])
                        });
                        let table =
                            Table::new(table_rows, [Constraint::Min(12), Constraint::Length(1)])
                                .header(Row::new(vec!["Model", "A"]))
                                .column_spacing(1);
                        let mut table_state = TableState::default();
                        table_state.select(Some(self.state.model_picker_selection));
                        frame.render_stateful_widget(table, rows[1], &mut table_state);
                    }
                }
            })
            .map_err(|err| format!("TUI render failed: {err}"))?;

        let cursor_style = cursor_style_for_mode(self.state.input_mode);
        let _ = crossterm::execute!(std::io::stdout(), cursor_style);

        Ok(())
    }

    #[cfg(test)]
    pub(super) fn main_pane_rects_for_height(
        main_height: u16,
    ) -> (
        ratatui::layout::Rect,
        ratatui::layout::Rect,
        ratatui::layout::Rect,
        ratatui::layout::Rect,
    ) {
        render_frame::main_pane_rects_for_height(main_height)
    }

    #[cfg(test)]
    pub fn pump_once(&mut self, event_source: &mut impl TerminalEventSource) {
        self.poll_terminal_event(event_source);
        self.drain_transport();
    }
}

#[cfg(test)]
pub(super) fn modal_frame_uses_rounded_border_style_for_test() -> bool {
    true
}

#[cfg(test)]
pub(super) fn modal_open_state_applies_dimmed_backdrop_for_test(state: &AppState) -> bool {
    state.command_palette_open || state.info_panel.is_some() || state.model_picker_open
}

#[cfg(test)]
pub(super) fn inline_model_picker_modal_respects_border_and_backdrop_policy_for_test(
    state: &AppState,
) -> bool {
    state.model_picker_open && modal_frame_uses_rounded_border_style_for_test()
}

#[cfg(test)]
pub(super) fn model_picker_empty_state_message_for_test() -> &'static str {
    MODEL_PICKER_EMPTY_STATE_MESSAGE
}

#[cfg(test)]
pub(super) fn permission_prompt_transcript_lines_for_test(
    prompt: &crate::agent::ui::tui::state::PermissionPrompt,
) -> Vec<crate::agent::ui::tui::state::TranscriptLine> {
    permission_prompt_transcript_lines(prompt)
}

#[cfg(test)]
pub(super) fn transcript_with_permission_prompt_for_render_for_test(
    state: &AppState,
) -> Vec<crate::agent::ui::tui::state::TranscriptLine> {
    transcript_with_permission_prompt_for_render(state)
}

#[cfg(test)]
pub(super) fn required_permission_prompt_line_index_for_render_for_test(
    state: &AppState,
    transcript_len: usize,
) -> Option<usize> {
    required_permission_prompt_line_index_for_render(state, transcript_len)
}

#[cfg(test)]
pub(super) fn required_permission_prompt_line_for_window_selection_for_test(
    state: &AppState,
    transcript_len: usize,
) -> Option<usize> {
    required_permission_prompt_line_for_window_selection(state, transcript_len)
}

#[cfg(test)]
pub(super) fn permission_prompt_footer_text_for_test(state: &AppState) -> Option<String> {
    permission_prompt_footer_text_for_state(state)
}

#[cfg(test)]
pub(super) fn transcript_pane_regions_for_test(
    transcript_area: Rect,
    reserve_footer_row: bool,
) -> (Rect, Option<Rect>) {
    transcript_pane_regions(transcript_area, reserve_footer_row)
}

fn help_panel_lines() -> (&'static str, Vec<Line<'static>>) {
    (
        "Help",
        crate::agent::ui::tui::markdown::project_markdown_to_lines(help_panel_markdown_source()),
    )
}

fn help_panel_markdown_source() -> &'static str {
    include_str!("help/help.md")
}

fn status_panel_lines(
    state: &AppState,
    active_model_identity: &str,
    input_backend_status: &str,
    last_input_poll_status: &str,
    last_input_error: Option<&str>,
) -> (&'static str, Vec<Line<'static>>) {
    let lines = build_status_lines(
        state,
        active_model_identity,
        input_backend_status,
        last_input_poll_status,
        last_input_error,
    )
    .into_iter()
    .map(Line::from)
    .collect();
    ("Status", lines)
}

#[cfg(test)]
pub(super) fn help_panel_lines_for_test() -> (&'static str, Vec<Line<'static>>) {
    help_panel_lines()
}

#[cfg(test)]
pub(super) fn help_panel_max_scroll_for_test(lines: &[Line<'_>], viewport_height: u16) -> usize {
    help_panel_max_scroll(lines, viewport_height, 80)
}

#[cfg(test)]
pub(super) fn help_panel_overflow_cue_for_test(
    lines: &[Line<'_>],
    viewport_height: u16,
    scroll: usize,
) -> Option<String> {
    help_panel_overflow_cue(lines, viewport_height, 80, scroll)
}

#[cfg(test)]
pub(super) fn help_panel_visible_window_for_test(
    lines: &[Line<'_>],
    viewport_height: u16,
    scroll: usize,
) -> Vec<Line<'static>> {
    help_panel_visible_window(lines, 80, scroll, viewport_height.max(1) as usize)
}

#[cfg(test)]
pub(super) fn status_panel_lines_for_test(
    state: &AppState,
    active_model_identity: &str,
    input_backend_status: &str,
    last_input_poll_status: &str,
    last_input_error: Option<&str>,
) -> (&'static str, Vec<Line<'static>>) {
    status_panel_lines(
        state,
        active_model_identity,
        input_backend_status,
        last_input_poll_status,
        last_input_error,
    )
}

#[cfg(test)]
pub(super) fn skills_panel_lines_for_test(state: &AppState) -> (&'static str, Vec<Line<'static>>) {
    skills_panel_lines(state)
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct McpTableModelForTest {
    pub columns: Vec<String>,
    pub rows: Vec<[String; 3]>,
    pub selected: Option<usize>,
    pub overflow_cue: Option<String>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct McpSelectedDetailsForTest {
    pub server_line: String,
    pub error_line: String,
    pub tools_line: String,
}

#[cfg(test)]
pub(super) fn mcp_selected_details_for_test(state: &AppState) -> Option<McpSelectedDetailsForTest> {
    mcp_selected_details(state).map(|details| McpSelectedDetailsForTest {
        server_line: details.server_line,
        error_line: details.error_line,
        tools_line: details.tools_line,
    })
}

#[cfg(test)]
pub(super) fn mcp_selected_details_lines_for_test(
    state: &AppState,
    details_height: u16,
    details_width: u16,
) -> Vec<String> {
    mcp_selected_details_lines(state, details_height, details_width)
        .into_iter()
        .map(|line| line.to_string())
        .collect()
}

#[cfg(test)]
pub(super) fn mcp_details_height_for_inner_height_for_test(inner_height: u16) -> u16 {
    mcp_details_height_for_inner_height(inner_height)
}

#[cfg(test)]
pub(super) fn mcp_panel_controls_line_for_test() -> String {
    mcp_panel_controls_line().to_string()
}

#[cfg(test)]
pub(super) fn mcp_status_column_width_for_test() -> u16 {
    MCP_STATUS_COLUMN_WIDTH
}

#[cfg(test)]
pub(super) fn mcp_table_model_for_test(
    state: &AppState,
    popup_width: u16,
    popup_height: u16,
) -> McpTableModelForTest {
    let _ = popup_width;
    let model = mcp_table_model(state, popup_height);
    McpTableModelForTest {
        columns: model.columns,
        rows: model.rows,
        selected: model.selected,
        overflow_cue: model.overflow_cue,
    }
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommandPaletteTableModelForTest {
    pub query_line: String,
    pub columns: Vec<String>,
    pub rows: Vec<[String; 3]>,
    pub selected: Option<usize>,
    pub overflow_cue: Option<String>,
}

#[cfg(test)]
pub(super) fn command_palette_table_model_for_test(
    state: &AppState,
    popup_width: u16,
    popup_height: u16,
) -> CommandPaletteTableModelForTest {
    let model = command_palette_table_model(state, popup_width, popup_height);
    CommandPaletteTableModelForTest {
        query_line: model.query_line,
        columns: model.columns,
        rows: model.rows,
        selected: model.selected,
        overflow_cue: model.overflow_cue,
    }
}

#[cfg(test)]
pub(super) fn inline_slash_lines_for_test(state: &AppState) -> Vec<String> {
    inline_slash_lines_for_render(state)
        .into_iter()
        .map(|line| line.to_string())
        .collect()
}

#[cfg(test)]
pub(super) fn compact_status_line_for_test(
    state: &AppState,
    active_model_identity: &str,
    input_backend_status: &str,
    last_input_poll_status: &str,
    last_input_error: Option<&str>,
) -> String {
    compact_status_line(
        state,
        active_model_identity,
        None,
        input_backend_status,
        last_input_poll_status,
        last_input_error,
        120,
    )
}

#[cfg(test)]
pub(super) fn lane_2_status_line_for_test(state: &AppState, width: usize) -> String {
    lane_2_status_line(state, width)
}

#[cfg(test)]
pub(super) fn status_lines_for_test(
    state: &AppState,
    active_model_identity: &str,
    input_backend_status: &str,
    last_input_poll_status: &str,
    last_input_error: Option<&str>,
) -> Vec<String> {
    build_status_lines(
        state,
        active_model_identity,
        input_backend_status,
        last_input_poll_status,
        last_input_error,
    )
}

#[cfg(test)]
pub(super) fn visual_indicator_line_for_test(state: &AppState) -> Option<String> {
    visual_indicator_line(state)
}

#[cfg(test)]
pub(super) fn transcript_title_for_test(state: &AppState) -> String {
    transcript_title_for_render(state, state.transcript_preview.len())
}

#[cfg(test)]
pub(super) fn cursor_style_for_test(
    mode: crate::agent::ui::tui::state::InputMode,
) -> crossterm::cursor::SetCursorStyle {
    cursor_style_for_mode(mode)
}

#[cfg(test)]
pub(super) fn parse_persisted_tool_status_line_for_test(line: &str) -> Option<(&str, &str, bool)> {
    parse_persisted_tool_status_line(line)
}

#[cfg(test)]
pub(super) use transcript_window::visible_transcript_window;

#[cfg(test)]
pub(super) fn visible_transcript_window_for_render_for_test(
    transcript: &[crate::agent::ui::tui::state::TranscriptLine],
    visible_lines: usize,
    scroll_from_bottom: usize,
    follow_tail: bool,
    content_width: usize,
) -> (usize, Vec<crate::agent::ui::tui::state::TranscriptLine>) {
    transcript_window::visible_transcript_window_for_render_with_required_line(
        transcript,
        visible_lines,
        scroll_from_bottom,
        follow_tail,
        content_width,
        None,
        &vec![None; transcript.len()],
    )
}

#[cfg(test)]
pub(super) fn visible_transcript_window_for_render_with_required_line_for_test(
    transcript: &[crate::agent::ui::tui::state::TranscriptLine],
    visible_lines: usize,
    scroll_from_bottom: usize,
    follow_tail: bool,
    content_width: usize,
    required_line_index: Option<usize>,
) -> (usize, Vec<crate::agent::ui::tui::state::TranscriptLine>) {
    transcript_window::visible_transcript_window_for_render_with_required_line(
        transcript,
        visible_lines,
        scroll_from_bottom,
        follow_tail,
        content_width,
        required_line_index,
        &vec![None; transcript.len()],
    )
}

#[cfg(test)]
pub(super) fn visible_transcript_window_for_render_with_required_line_and_statuses_for_test(
    transcript: &[crate::agent::ui::tui::state::TranscriptLine],
    visible_lines: usize,
    scroll_from_bottom: usize,
    follow_tail: bool,
    content_width: usize,
    required_line_index: Option<usize>,
    line_statuses: &[Option<TranscriptLineStatus>],
) -> (usize, Vec<crate::agent::ui::tui::state::TranscriptLine>) {
    transcript_window::visible_transcript_window_for_render_with_required_line(
        transcript,
        visible_lines,
        scroll_from_bottom,
        follow_tail,
        content_width,
        required_line_index,
        line_statuses,
    )
}

const IN_PROGRESS_SPINNER_FRAMES: [&str; 10] = render_frame::IN_PROGRESS_SPINNER_FRAMES;

#[cfg(test)]
pub(super) fn indicator_style_for_status_for_test(status: TranscriptLineStatus) -> Style {
    indicator_style_for_status(status, &TuiTheme::default())
}

#[cfg(test)]
pub(super) fn transition_spacer_for_roles_for_test(
    previous: Option<TranscriptRole>,
    next: TranscriptRole,
) -> bool {
    should_insert_transition_spacer(previous, next)
}

#[cfg(test)]
pub(super) fn prompt_indicator_for_status_for_test(
    status: PromptStatus,
    now_millis: u128,
) -> &'static str {
    prompt_indicator_for_status(status, now_millis)
}

#[cfg(test)]
pub(super) fn render_transcript_lines_for_test(
    line: crate::agent::ui::tui::state::TranscriptLine,
    line_status: Option<TranscriptLineStatus>,
    now_millis: u128,
) -> Vec<Line<'static>> {
    render_transcript_lines(
        line,
        80,
        false,
        false,
        line_status,
        now_millis,
        &TuiTheme::default(),
    )
}

#[cfg(test)]
pub(super) fn render_transcript_lines_with_flags_for_test(
    line: crate::agent::ui::tui::state::TranscriptLine,
    line_status: Option<TranscriptLineStatus>,
    selected: bool,
    cursor_line: bool,
    width: usize,
    now_millis: u128,
) -> Vec<Line<'static>> {
    render_transcript_lines(
        line,
        width,
        selected,
        cursor_line,
        line_status,
        now_millis,
        &TuiTheme::default(),
    )
}

#[cfg(test)]
pub(super) fn lane_prefix_spans_for_test(
    role: TranscriptRole,
    cursor_line: bool,
) -> Vec<Span<'static>> {
    lane_prefix_spans(role, cursor_line, &TuiTheme::default())
}

#[cfg(test)]
pub(super) fn row_spans_for_test(
    line: crate::agent::ui::tui::state::TranscriptLine,
    line_status: Option<TranscriptLineStatus>,
    cursor_line: bool,
    selected: bool,
    now_millis: u128,
) -> Vec<Span<'static>> {
    build_row_spans(
        &line,
        line_status,
        cursor_line,
        selected,
        now_millis,
        &TuiTheme::default(),
        true,
    )
}

#[cfg(test)]
pub(super) fn input_line_for_test(state: &AppState) -> String {
    let _ = current_time_millis();
    state.input.buffer.clone()
}

#[cfg(test)]
pub(super) fn input_line_for_test_at_millis(state: &AppState, now_millis: u128) -> String {
    let _ = now_millis;
    state.input.buffer.clone()
}

#[cfg(test)]
pub(super) fn input_rows_with_prompt_for_test(state: &AppState, pane_width: u16) -> Vec<String> {
    let rows = wrapped_input_rows(&state.input.buffer, pane_width.saturating_sub(2) as usize);

    let mut lines = Vec::new();
    let prompt_prefix = input_prompt_prefix(state.input_mode);
    if let Some((first, rest)) = rows.split_first() {
        lines.push(format!("{prompt_prefix}{first}"));
        for row in rest {
            lines.push(format!("  {row}"));
        }
    }

    lines
}

pub struct TuiRuntimeRenderer<R, E>
where
    R: UiRenderer,
    E: TerminalEventSource,
{
    inner: R,
    coordinator: RuntimeCoordinator,
    event_source: E,
    live_terminal: Option<LiveTerminalUi>,
    tui_active: bool,
}

impl<R, E> TuiRuntimeRenderer<R, E>
where
    R: UiRenderer,
    E: TerminalEventSource,
{
    fn with_terminal_mode(
        inner: R,
        event_source: E,
        columns: u16,
        rows: u16,
        live_terminal: Option<LiveTerminalUi>,
        tui_active: bool,
    ) -> Self {
        Self {
            inner,
            coordinator: RuntimeCoordinator::new(columns, rows, Some(true)),
            event_source,
            live_terminal,
            tui_active,
        }
    }

    fn mark_render_failure(&mut self, error: String) {
        self.coordinator.state.status_line = error.clone();
        self.coordinator.fatal_error = Some(error);
        self.coordinator.quit_requested = true;
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn new(inner: R, event_source: E, columns: u16, rows: u16) -> Self {
        Self::with_terminal_mode(inner, event_source, columns, rows, None, false)
    }

    pub fn new_live(inner: R, event_source: E, columns: u16, rows: u16) -> Result<Self, String> {
        let live_terminal = LiveTerminalUi::new()?;
        Ok(Self::with_terminal_mode(
            inner,
            event_source,
            columns,
            rows,
            Some(live_terminal),
            true,
        ))
    }

    #[cfg(test)]
    pub fn new_tui_active_for_test(inner: R, event_source: E, columns: u16, rows: u16) -> Self {
        Self::with_terminal_mode(inner, event_source, columns, rows, None, true)
    }

    #[cfg(test)]
    pub fn coordinator(&self) -> &RuntimeCoordinator {
        &self.coordinator
    }

    pub fn take_cancel_requested(&self) -> bool {
        self.coordinator.take_cancel_requested()
    }

    pub(crate) fn set_active_model_identity(&mut self, active_model_identity: String) {
        self.coordinator
            .set_active_model_identity(active_model_identity);
    }

    pub(crate) fn set_mcp_lifecycle_projection(&mut self, projection: Vec<McpServerLifecycle>) {
        self.coordinator.set_mcp_lifecycle_projection(projection);
    }

    pub(crate) fn set_skills_projection(&mut self, skills: Vec<ProtocolDiscoverableSkill>) {
        self.coordinator.set_skills_projection(skills);
    }

    pub(crate) fn mark_skills_discovery_failed(&mut self) {
        self.coordinator.mark_skills_discovery_failed();
    }

    pub(crate) fn take_next_mcp_toggle_request(&mut self) -> Option<McpToggleRequest> {
        self.coordinator.take_next_mcp_toggle_request()
    }

    pub(crate) fn take_next_model_switch_request(&mut self) -> Option<String> {
        self.coordinator.take_next_model_switch_request()
    }

    pub(crate) fn take_next_permission_decision_submission(
        &mut self,
    ) -> Option<PermissionDecisionSubmission> {
        self.coordinator.take_next_permission_decision_submission()
    }

    pub(crate) fn set_mcp_server_state(
        &mut self,
        server_name: &str,
        state: McpServerUsabilityState,
    ) -> bool {
        self.coordinator.set_mcp_server_state(server_name, state)
    }

    pub(crate) fn set_mcp_server_state_with_details(
        &mut self,
        server_name: &str,
        state: McpServerUsabilityState,
        reason: Option<String>,
        llm_visible_mcp_tool_count: usize,
    ) -> bool {
        self.coordinator.set_mcp_server_state_with_details(
            server_name,
            state,
            reason,
            llm_visible_mcp_tool_count,
        )
    }

    pub(crate) fn set_llm_visible_mcp_tool_count(&mut self, count: usize) {
        self.coordinator.set_llm_visible_mcp_tool_count(count);
    }

    pub(crate) fn set_mcp_visible_tool_count_by_server_name(
        &mut self,
        server_name: &str,
        count: usize,
    ) {
        self.coordinator
            .set_mcp_visible_tool_count_by_server_name(server_name, count);
    }

    pub(crate) fn set_mcp_visible_tool_names_by_server_name(
        &mut self,
        server_name: &str,
        names: Vec<String>,
    ) {
        self.coordinator
            .set_mcp_visible_tool_names_by_server_name(server_name, names);
    }

    pub(crate) fn set_context_window_max_tokens(&mut self, max_tokens: Option<u64>) {
        self.coordinator.set_context_window_max_tokens(max_tokens);
    }

    pub(crate) fn set_model_picker_options(&mut self, options: Vec<ModelPickerOption>) {
        self.coordinator.set_model_picker_options(options);
    }

    pub(crate) fn set_repo_branch_caller_cwd(&mut self, caller_cwd: Option<PathBuf>) {
        self.coordinator.set_repo_branch_caller_cwd(caller_cwd);
    }

    pub(crate) fn fatal_error(&self) -> Option<&str> {
        self.coordinator.fatal_error()
    }

    pub(crate) fn hydrate_transcript_from_messages(
        &mut self,
        messages: impl IntoIterator<Item = UiMessageSnapshot>,
    ) {
        self.coordinator.hydrate_transcript_from_messages(messages);
    }

    pub fn pump_terminal_once(&mut self) {
        self.coordinator.poll_terminal_event(&mut self.event_source);
        self.coordinator.drain_transport();
        if let Err(error) = self.coordinator.render_frame(&mut self.live_terminal) {
            self.mark_render_failure(error);
        }
    }

    pub(crate) fn take_submitted_prompt(&mut self) -> Option<String> {
        self.coordinator.take_submitted_prompt()
    }

    pub(crate) fn take_next_model_picker_launch_request(&mut self) -> bool {
        self.coordinator.take_next_model_picker_launch_request()
    }

    pub(crate) fn execute_shared_ui_action(&mut self, action: SharedUiAction) -> bool {
        self.coordinator.execute_shared_ui_action(action)
    }

    pub fn quit_requested(&self) -> bool {
        self.coordinator.quit_requested()
    }
}

impl<R, E> UiRenderer for TuiRuntimeRenderer<R, E>
where
    R: UiRenderer,
    E: TerminalEventSource,
{
    fn emit(&mut self, event: &UiEvent) {
        self.coordinator.poll_terminal_event(&mut self.event_source);
        self.coordinator.enqueue_ui_event(event.clone());
        self.coordinator.drain_transport();
        if let Err(error) = self.coordinator.render_frame(&mut self.live_terminal) {
            self.mark_render_failure(error);
        }
        if !self.tui_active {
            self.inner.emit(event);
        }
    }

    fn flush(&mut self) {
        self.inner.flush();
    }
}

#[derive(Debug)]
pub enum RuntimeRunError<E> {
    Enter(TerminalLifecycleError),
    Run(RestoreRunError<E, TerminalLifecycleError>),
}

pub fn run_with_terminal_restore<B, T, E, F>(
    lifecycle: &mut TerminalLifecycle<B>,
    run: F,
) -> Result<T, RuntimeRunError<E>>
where
    B: TerminalBackend,
    F: FnOnce() -> Result<T, E>,
{
    lifecycle.enter().map_err(RuntimeRunError::Enter)?;
    run_with_restore(lifecycle, run).map_err(RuntimeRunError::Run)
}

pub struct AnsiTerminalBackend<W>
where
    W: Write,
{
    writer: W,
}

impl<W> AnsiTerminalBackend<W>
where
    W: Write,
{
    pub fn new(writer: W) -> Self {
        Self { writer }
    }
}

impl<W> TerminalBackend for AnsiTerminalBackend<W>
where
    W: Write,
{
    fn enable_raw_mode(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::terminal::enable_raw_mode().map_err(|err| {
            TerminalLifecycleError::new(
                crate::agent::ui::tui::platform::terminal::TerminalAction::EnableRawMode,
                err.to_string(),
            )
        })
    }

    fn disable_raw_mode(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::terminal::disable_raw_mode().map_err(|err| {
            TerminalLifecycleError::new(
                crate::agent::ui::tui::platform::terminal::TerminalAction::DisableRawMode,
                err.to_string(),
            )
        })
    }

    fn enter_alt_screen(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::terminal::EnterAlternateScreen).map_err(|err| {
            TerminalLifecycleError::new(
                crate::agent::ui::tui::platform::terminal::TerminalAction::EnterAltScreen,
                err.to_string(),
            )
        })
    }

    fn leave_alt_screen(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::terminal::LeaveAlternateScreen).map_err(|err| {
            TerminalLifecycleError::new(
                crate::agent::ui::tui::platform::terminal::TerminalAction::LeaveAltScreen,
                err.to_string(),
            )
        })
    }

    fn hide_cursor(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::cursor::Hide).map_err(|err| {
            TerminalLifecycleError::new(
                crate::agent::ui::tui::platform::terminal::TerminalAction::HideCursor,
                err.to_string(),
            )
        })
    }

    fn show_cursor(&mut self) -> Result<(), TerminalLifecycleError> {
        crossterm::execute!(self.writer, crossterm::cursor::Show).map_err(|err| {
            TerminalLifecycleError::new(
                crate::agent::ui::tui::platform::terminal::TerminalAction::ShowCursor,
                err.to_string(),
            )
        })
    }
}

struct LiveTerminalUi {
    terminal: Terminal<CrosstermBackend<std::io::Stderr>>,
}

impl LiveTerminalUi {
    fn new() -> Result<Self, String> {
        let backend = CrosstermBackend::new(std::io::stderr());
        let mut terminal = Terminal::new(backend)
            .map_err(|err| format!("failed to initialize ratatui terminal: {err}"))?;
        terminal
            .clear()
            .map_err(|err| format!("failed to clear ratatui terminal: {err}"))?;
        Ok(Self { terminal })
    }
}
