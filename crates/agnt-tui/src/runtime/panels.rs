use ratatui::{
    layout::{Constraint, Rect},
    text::{Line, Span, Text},
    widgets::Paragraph,
};
use unicode_width::UnicodeWidthStr;

use crate::{
    rendering::theme::TuiTheme,
    runtime::{render::frame::picker_column_budgets, status::format::ellipsize},
    state::{AppState, PickerPayload},
};

pub(super) fn render_permission_controls(frame: &mut ratatui::Frame, area: Rect, theme: &TuiTheme) {
    let controls = Line::from(vec![
        Span::styled("[a]", theme.status_running),
        Span::raw(" allow once  "),
        Span::styled("[A]", theme.status_running),
        Span::raw(" allow always  "),
        Span::styled("[d]", theme.status_running),
        Span::raw(" deny"),
    ]);
    let widget = Paragraph::new(Text::from(vec![controls]));
    frame.render_widget(widget, area);
}

pub(super) fn help_panel_max_scroll(lines: &[Line<'_>], viewport_height: u16) -> usize {
    let visible_rows = viewport_height.max(1) as usize;
    lines.len().saturating_sub(visible_rows)
}

pub(super) fn help_panel_overflow_cue(
    lines: &[Line<'_>],
    viewport_height: u16,
    scroll: usize,
) -> String {
    let keys = scroll_panel_action_keys();
    let visible_rows = viewport_height.max(1) as usize;
    let total_rows = lines.len();
    if total_rows <= visible_rows {
        return keys.to_string();
    }

    let max_scroll = total_rows.saturating_sub(visible_rows);
    let current = scroll.min(max_scroll);
    let start = current.saturating_add(1);
    let end = current.saturating_add(visible_rows).min(total_rows);
    format!("{keys} · {start}-{end} / {total_rows}")
}

/// A popup title: the base label with its optional navigation keys and
/// optional overflow cue joined into `{base} ({keys} · {cue})`, bounded to the
/// popup's title row. The row spans the popup width minus the two border
/// corners, and a block title past that width hard-clips without a marker.
pub(super) fn popup_title(
    base: &str,
    keys: Option<&str>,
    cue: Option<&str>,
    popup_width: u16,
) -> String {
    let hint = match (keys, cue) {
        (Some(keys), Some(cue)) => Some(format!("{keys} · {cue}")),
        (Some(keys), None) => Some(keys.to_string()),
        (None, Some(cue)) => Some(cue.to_string()),
        (None, None) => None,
    };
    let title = match hint {
        Some(hint) => format!("{base} ({hint})"),
        None => base.to_string(),
    };
    ellipsize(&title, popup_width.saturating_sub(2) as usize)
}

/// The scroll-panel title (Help/Status/Skills): the panel label with its
/// position cue and no keys hint.
pub(super) fn scroll_panel_title(base: &str, cue: &str, popup_width: u16) -> String {
    popup_title(base, None, Some(cue), popup_width)
}

/// Wrap every panel line to `width` display columns so the lines returned here
/// are exactly the rows the panel paints: the render drops `Paragraph`'s own
/// wrap and the scroll math counts these lines.
pub(super) fn wrap_panel_lines(lines: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .flat_map(|line| wrap_panel_line(line, width))
        .collect()
}

/// Wrap one rendered line to `width` display columns, keeping each span's
/// style: the wrapped rows are byte substrings of the joined span text, so the
/// forward scan slices the original spans at each row's range.
fn wrap_panel_line(line: Line<'static>, width: u16) -> Vec<Line<'static>> {
    let width_cells = usize::from(width.max(1));
    if line.width() <= width_cells {
        return vec![line];
    }

    let text = line.to_string();
    let mut rows = Vec::new();
    let mut cursor = 0usize;
    for row in wrap_at_ascii_spaces(&text, width) {
        if row.is_empty() {
            rows.push(Line::default());
            continue;
        }
        let Some(start) = text[cursor..].find(&row).map(|rel| cursor + rel) else {
            rows.push(Line::from(row));
            continue;
        };
        let end = start + row.len();
        cursor = end;
        rows.push(Line::from(slice_line_spans(&line.spans, start, end)));
    }
    rows
}

/// The spans covering byte range `[start, end)` of the joined span text.
fn slice_line_spans(spans: &[Span<'static>], start: usize, end: usize) -> Vec<Span<'static>> {
    let mut result = Vec::new();
    let mut offset = 0usize;
    for span in spans {
        let span_start = offset;
        let span_end = offset + span.content.len();
        offset = span_end;
        let from = start.max(span_start);
        let to = end.min(span_end);
        if from >= to {
            continue;
        }
        result.push(Span::styled(
            span.content[from - span_start..to - span_start].to_string(),
            span.style,
        ));
    }
    result
}

/// Navigation keys every scrollable panel shows in its title.
pub(super) fn scroll_panel_action_keys() -> &'static str {
    "↑/↓ · PgUp/PgDn · Esc"
}

/// Navigation keys every picker shows in its title.
pub(super) fn picker_action_keys() -> &'static str {
    "↑/↓ · Enter · Esc"
}

/// The hint line a picker shows above its table: the literal prefix and the
/// query the user has typed so far.
pub(super) fn picker_query_line(query: &str) -> String {
    format!("Query: {query}")
}

/// The title of the inline-slash suggestions popup under the input box.
pub(super) fn inline_slash_title() -> String {
    format!("Commands ({})", picker_action_keys())
}

pub(super) fn command_palette_action_keys() -> &'static str {
    picker_action_keys()
}

/// The command palette title: its label, the global keys hint, and the
/// optional overflow cue.
pub(super) fn command_palette_title(overflow_cue: Option<&str>, popup_width: u16) -> String {
    popup_title(
        "Command Palette",
        Some(command_palette_action_keys()),
        overflow_cue,
        popup_width,
    )
}

/// The MCPs panel title: the panel label with its optional overflow cue and no
/// keys hint.
pub(super) fn mcp_panel_title(overflow_cue: Option<&str>, popup_width: u16) -> String {
    popup_title("MCPs", None, overflow_cue, popup_width)
}

pub(super) const MODEL_PICKER_EMPTY_STATE_MESSAGE: &str =
    "No models available in cached startup config.";
pub(super) const AGENT_PICKER_EMPTY_STATE_MESSAGE: &str =
    "No agent personas found. Create .agents/<name>.md files.";
pub(super) const THEME_PICKER_EMPTY_STATE_MESSAGE: &str = "No themes available";

pub(crate) fn skills_panel_lines(
    state: &AppState,
    width: u16,
) -> (&'static str, Vec<Line<'static>>) {
    if state.status.mcp.skills_discovery_failed() {
        return (
            "Skills",
            wrap_panel_lines(
                vec![Line::from(
                    "Skills discovery unavailable. Showing no skills.",
                )],
                width,
            ),
        );
    }

    if state.status.mcp.discoverable_skills().is_empty() {
        return (
            "Skills",
            wrap_panel_lines(vec![Line::from("No discoverable skills available.")], width),
        );
    }

    let mut lines = vec![Line::from("Discoverable skills")];
    lines.extend(state.status.mcp.discoverable_skills().iter().map(|skill| {
        let suffix = skill
            .description
            .as_ref()
            .map(|desc| format!(" — {desc}"))
            .unwrap_or_default();
        Line::from(format!("- {} ({}){suffix}", skill.name, skill.source))
    }));
    ("Skills", wrap_panel_lines(lines, width))
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
    pub(super) controls_line: String,
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
    let server = state.status.mcp.selected_mcp_server()?;
    let reason = state
        .status
        .mcp
        .failed_mcp_servers_with_reasons()
        .into_iter()
        .find(|(name, _)| *name == server.name.as_str())
        .and_then(|(_, reason)| reason)
        .map(str::trim)
        .filter(|reason| !reason.is_empty());
    let mut tool_names = state
        .status
        .mcp
        .mcp_visible_tool_names_for_server_name(server.name.as_str());
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

/// The rows the MCP details pane paints: each detail line wrapped to `width`
/// display columns and the whole stack cut to the ladder's `max_rows` budget.
/// Rendering these rows without `Paragraph`'s own wrap keeps measurement and
/// rendering in lockstep — the rows returned here are exactly the rows the pane
/// paints, so the details rect can be sized to them.
pub(crate) fn mcp_details_rows(
    lines: &[Line<'static>],
    width: u16,
    max_rows: u16,
) -> Vec<Line<'static>> {
    let max_rows = usize::from(max_rows);
    let width_cells = usize::from(width);
    let mut rows: Vec<Line<'static>> = Vec::new();
    for line in lines {
        if rows.len() >= max_rows {
            break;
        }
        // A fitting line renders verbatim: the tool rows carry a continuation
        // indent the wrap would trim, and re-wrapping them buys nothing.
        if line.width() <= width_cells {
            rows.push(line.clone());
            continue;
        }
        // An over-wide line is wrapped by hand so the leading indent (a tool
        // row's continuation offset) survives on every wrapped row and the body
        // wraps in the width left after it.
        let text = line.to_string();
        let indent_len = text
            .len()
            .saturating_sub(text.trim_start_matches(' ').len());
        let (indent, body) = text.split_at(indent_len);
        let body_width = width.saturating_sub(indent_len as u16);
        rows.extend(
            wrap_at_ascii_spaces(body, body_width)
                .into_iter()
                .map(|row| Line::from(format!("{indent}{row}"))),
        );
    }
    rows.truncate(max_rows);
    rows
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

pub(crate) fn picker_details_height_for_inner_height(inner_height: u16) -> u16 {
    match inner_height {
        0..=7 => 0,
        8..=12 => 2,
        _ => 3,
    }
}

/// The greedy first-fit wrap shared by the pickers' footer and the MCP
/// details rows: breaks on ASCII spaces only and splits overlong words at the
/// column boundary — the same fill ratatui's `Wrap` uses — so text with no
/// spaces (a model identity, a long error) keeps every character across rows
/// instead of breaking at a `/` or `-` separator.
pub(crate) fn wrap_at_ascii_spaces(text: &str, width: u16) -> Vec<String> {
    textwrap::wrap(
        text,
        textwrap::Options::new(usize::from(width.max(1)))
            .word_separator(textwrap::WordSeparator::AsciiSpace)
            .word_splitter(textwrap::WordSplitter::NoHyphenation),
    )
    .into_iter()
    .map(|line| line.into_owned())
    .collect()
}

pub(crate) fn mcp_panel_controls_line() -> &'static str {
    "↑/↓ select · type to filter · Enter/Space toggle · Esc close"
}

/// The MCP table's column constraints: the server name, the visible tool count,
/// and the status icon. Shared with the renderer so the ellipsize budgets come
/// from the constraints the table resolves.
pub(super) const MCP_TABLE_COLUMNS: [Constraint; 3] = [
    Constraint::Min(18),
    Constraint::Length(14),
    Constraint::Length(MCP_STATUS_COLUMN_WIDTH),
];

/// Column spacing of the MCP table.
pub(super) const MCP_TABLE_COLUMN_SPACING: u16 = 1;

/// The selection symbol every picker table paints in the selection column it
/// carves out before it lays out the data columns; its width is that column's
/// width.
pub(crate) const PICKER_HIGHLIGHT_SYMBOL: &str = "❯ ";

/// The MCP table's highlight symbol; its width is the selection column the
/// table carves out before it lays out the data columns.
pub(super) const MCP_TABLE_HIGHLIGHT_SYMBOL: &str = PICKER_HIGHLIGHT_SYMBOL;

pub(super) fn mcp_table_model(
    state: &AppState,
    table_width: u16,
    table_height: u16,
) -> McpTableModel {
    // The table carves out the selection column and the column spacing before
    // it resolves the constraints, so derive the Name cell budget from the
    // width the solver gives that column instead of assuming the fixed
    // overhead fits: below a 42-column table the Name minimum pins that column
    // and the budget follows it down instead of undershooting.
    let budgets = picker_column_budgets(
        table_width,
        UnicodeWidthStr::width(MCP_TABLE_HIGHLIGHT_SYMBOL) as u16,
        MCP_TABLE_COLUMN_SPACING,
        &MCP_TABLE_COLUMNS,
    );
    let name_budget = budgets.first().copied().unwrap_or_default();
    let columns = vec![
        "Name".to_string(),
        "Visible tools".to_string(),
        "Status".to_string(),
    ];
    let query = state.status.mcp.mcp_panel_query();
    let all_rows = state
        .status
        .mcp
        .mcp_servers_filtered()
        .iter()
        .map(|server| {
            let visible_tools = state
                .status
                .mcp
                .mcp_visible_tool_count_for_server_name(server.name.as_str())
                .to_string();
            [
                ellipsize(&server.name, name_budget),
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
                .status
                .mcp
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
            "↑/↓ select · Enter/Space toggle · Esc close · {}-{} / {}",
            start,
            end,
            all_rows.len()
        ))
    } else {
        None
    };

    // The controls line renders into a one-row rect with no wrap, so it must
    // fit the rendered width itself — an over-wide line hard-clips without a
    // marker at the popup's minimum width.
    let controls_line = if query.is_empty() {
        ellipsize(mcp_panel_controls_line(), table_width as usize)
    } else {
        ellipsize(&picker_query_line(query), table_width as usize)
    };

    McpTableModel {
        controls_line,
        columns,
        rows,
        selected,
        overflow_cue,
    }
}

/// The command palette table's column constraints: the Action label and the
/// Summary text. Shared with the renderer so the ellipsize budgets come from
/// the constraints the table resolves.
pub(super) const COMMAND_PALETTE_COLUMNS: [Constraint; 2] =
    [Constraint::Length(8), Constraint::Min(12)];

/// Column spacing of the command palette table.
pub(super) const COMMAND_PALETTE_COLUMN_SPACING: u16 = 2;

/// The command palette's highlight symbol; its width is the selection column
/// the table carves out before it lays out the data columns.
pub(super) const COMMAND_PALETTE_HIGHLIGHT_SYMBOL: &str = PICKER_HIGHLIGHT_SYMBOL;

pub(super) fn command_palette_table_model(
    state: &AppState,
    table_width: u16,
    popup_height: u16,
) -> CommandPaletteTableModel {
    let Some(picker_state) = state.picker.active_state() else {
        return CommandPaletteTableModel {
            query_line: String::new(),
            columns: Vec::new(),
            rows: Vec::new(),
            selected: None,
            overflow_cue: None,
        };
    };
    let actions = picker_state.filtered();
    // The table carves out the selection column and the column spacing before
    // it resolves the constraints, so derive each cell budget from the width
    // the solver gives that column instead of assuming the constraints fit:
    // below a 24-column table the Summary minimum pins that column and the
    // Action column takes the remainder.
    let budgets = picker_column_budgets(
        table_width,
        UnicodeWidthStr::width(COMMAND_PALETTE_HIGHLIGHT_SYMBOL) as u16,
        COMMAND_PALETTE_COLUMN_SPACING,
        &COMMAND_PALETTE_COLUMNS,
    );
    let action_budget = budgets.first().copied().unwrap_or_default();
    let summary_budget = budgets.get(1).copied().unwrap_or_default();
    let columns = vec!["Action".to_string(), "Summary".to_string()];

    let all_rows = actions
        .iter()
        .map(|opt| {
            let action = match &opt.payload {
                PickerPayload::Command(a) => *a,
                _ => unreachable!(),
            };
            [
                ellipsize(action.label(), action_budget),
                ellipsize(action.summary(), summary_budget),
                String::new(),
            ]
        })
        .collect::<Vec<_>>();

    let selected_global = if all_rows.is_empty() {
        None
    } else {
        Some(picker_state.selection.min(all_rows.len().saturating_sub(1)))
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
        Some(format!("{}-{} / {}", start, end, all_rows.len()))
    } else {
        None
    };

    CommandPaletteTableModel {
        query_line: picker_query_line(&picker_state.query),
        columns,
        rows,
        selected,
        overflow_cue,
    }
}
