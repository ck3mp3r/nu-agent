use super::*;

pub(super) fn help_panel_lines() -> (&'static str, Vec<Line<'static>>) {
    (
        "Help",
        crate::markdown::project_markdown_to_lines(help_panel_markdown_source()),
    )
}

pub(super) fn help_panel_markdown_source() -> &'static str {
    include_str!("help/help.md")
}

pub(super) fn status_panel_lines(
    state: &AppState,
    active_model_identity: &str,
) -> (&'static str, Vec<Line<'static>>) {
    let lines = build_status_lines(state, active_model_identity)
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
) -> (&'static str, Vec<Line<'static>>) {
    status_panel_lines(state, active_model_identity)
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
    active_model_identity: &str,
    now_millis: Option<u128>,
) -> String {
    compact_status_line(active_model_identity, None, now_millis, 120)
}

#[cfg(test)]
pub(super) fn lane_2_status_line_for_test(state: &AppState, width: usize) -> String {
    lane_2_status_line(state, width)
}

#[cfg(test)]
pub(super) fn status_lines_for_test(state: &AppState, active_model_identity: &str) -> Vec<String> {
    build_status_lines(state, active_model_identity)
}

#[cfg(test)]
pub(super) fn cursor_style_for_test(
    mode: crate::state::InputMode,
) -> crossterm::cursor::SetCursorStyle {
    mode.cursor_style()
}

#[cfg(test)]
pub(super) fn status_indicator_for_test(now_millis: Option<u128>) -> &'static str {
    status::status_indicator_for_test(now_millis)
}

#[cfg(test)]
pub(super) fn parse_persisted_tool_status_line_for_test(line: &str) -> Option<(&str, &str, bool)> {
    parse_persisted_tool_status_line(line)
}

#[cfg(test)]
pub(super) fn transition_spacer_for_roles_for_test(
    previous: Option<TranscriptRole>,
    next: TranscriptRole,
) -> bool {
    use nu_agent_core::transcript::ir::Role;
    let prev_role = previous.map(|r| match r {
        TranscriptRole::User => Role::User,
        TranscriptRole::Assistant => Role::Assistant,
        TranscriptRole::System => Role::System,
        TranscriptRole::Compaction => Role::Compaction,
        TranscriptRole::Tool => Role::Tool,
        TranscriptRole::ToolDisplay => Role::ToolDisplay,
        TranscriptRole::Separator => Role::Separator,
    });
    let next_role = match next {
        TranscriptRole::User => Role::User,
        TranscriptRole::Assistant => Role::Assistant,
        TranscriptRole::System => Role::System,
        TranscriptRole::Compaction => Role::Compaction,
        TranscriptRole::Tool => Role::Tool,
        TranscriptRole::ToolDisplay => Role::ToolDisplay,
        TranscriptRole::Separator => Role::Separator,
    };
    crate::state::transcript::needs_spacer(prev_role.as_ref(), &next_role)
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
