use super::*;
pub(crate) fn help_panel_max_scroll_for_test(lines: &[Line<'_>], viewport_height: u16) -> usize {
    crate::runtime::panels::help_panel_max_scroll(lines, viewport_height, 80)
}

pub(crate) fn help_panel_overflow_cue_for_test(
    lines: &[Line<'_>],
    viewport_height: u16,
    scroll: usize,
) -> Option<String> {
    crate::runtime::panels::help_panel_overflow_cue(lines, viewport_height, 80, scroll)
}

pub(crate) fn help_panel_visible_window_for_test(
    lines: &[Line<'_>],
    viewport_height: u16,
    scroll: usize,
) -> Vec<Line<'static>> {
    crate::runtime::panels_test::help_panel_visible_window(
        lines,
        80,
        scroll,
        viewport_height.max(1) as usize,
    )
}

/// Returns the effective (clamped) scroll offset that would be passed to
/// `render_scroll_text_panel` for the Help panel given a terminal area and
/// a requested scroll offset. The clamp uses `help_panel_max_scroll`, which
/// accounts for word-wrap using the actual content width.
///
/// Status and Skills panels share the same clamp logic; only Help additionally
/// shows an overflow cue in the title. The cue is omitted for Status/Skills
/// because those panels are short and rarely scroll in practice — add a cue
/// there if that assumption changes.
pub(crate) fn help_panel_scroll_offset_for_test(
    viewport_height: u16,
    viewport_width: u16,
    requested_scroll: usize,
) -> usize {
    let (_title, lines) = crate::runtime::status_help::help_panel_lines();
    requested_scroll.min(crate::runtime::panels::help_panel_max_scroll(
        &lines,
        viewport_height,
        viewport_width,
    ))
}

pub(crate) fn status_panel_scroll_offset_for_test(
    state: &AppState,
    active_model_identity: &str,
    viewport_height: u16,
    viewport_width: u16,
    requested_scroll: usize,
) -> usize {
    let (_title, lines) =
        crate::runtime::status_help::status_panel_lines(state, active_model_identity);
    requested_scroll.min(crate::runtime::panels::help_panel_max_scroll(
        &lines,
        viewport_height,
        viewport_width,
    ))
}

pub(crate) fn skills_panel_scroll_offset_for_test(
    state: &AppState,
    viewport_height: u16,
    viewport_width: u16,
    requested_scroll: usize,
) -> usize {
    let (_title, lines) = crate::runtime::panels::skills_panel_lines(state);
    requested_scroll.min(crate::runtime::panels::help_panel_max_scroll(
        &lines,
        viewport_height,
        viewport_width,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpTableModelForTest {
    pub columns: Vec<String>,
    pub rows: Vec<[String; 3]>,
    pub selected: Option<usize>,
    pub overflow_cue: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpSelectedDetailsForTest {
    pub server_line: String,
    pub error_line: String,
    pub tools_line: String,
}

pub(crate) fn mcp_selected_details_for_test(state: &AppState) -> Option<McpSelectedDetailsForTest> {
    crate::runtime::panels::mcp_selected_details(state).map(|details| McpSelectedDetailsForTest {
        server_line: details.server_line,
        error_line: details.error_line,
        tools_line: details.tools_line,
    })
}

pub(crate) fn mcp_selected_details_lines_for_test(
    state: &AppState,
    details_height: u16,
    details_width: u16,
) -> Vec<String> {
    crate::runtime::panels::mcp_selected_details_lines(state, details_height, details_width)
        .into_iter()
        .map(|line| line.to_string())
        .collect()
}

pub(crate) fn mcp_table_model_for_test(
    state: &AppState,
    popup_width: u16,
    popup_height: u16,
) -> McpTableModelForTest {
    let _ = popup_width;
    let model = crate::runtime::panels::mcp_table_model(state, popup_height);
    McpTableModelForTest {
        columns: model.columns,
        rows: model.rows,
        selected: model.selected,
        overflow_cue: model.overflow_cue,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommandPaletteTableModelForTest {
    pub query_line: String,
    pub columns: Vec<String>,
    pub rows: Vec<[String; 3]>,
    pub selected: Option<usize>,
    pub overflow_cue: Option<String>,
}

pub(crate) fn command_palette_table_model_for_test(
    state: &AppState,
    popup_width: u16,
    popup_height: u16,
) -> CommandPaletteTableModelForTest {
    let model =
        crate::runtime::panels::command_palette_table_model(state, popup_width, popup_height);
    CommandPaletteTableModelForTest {
        query_line: model.query_line,
        columns: model.columns,
        rows: model.rows,
        selected: model.selected,
        overflow_cue: model.overflow_cue,
    }
}

pub(crate) fn inline_slash_lines_for_test(state: &AppState) -> Vec<String> {
    crate::runtime::status_help::inline_slash_lines_for_render(state)
        .into_iter()
        .map(|line| line.to_string())
        .collect()
}

pub(crate) fn compact_status_line_for_test(
    active_model_identity: &str,
    now_millis: Option<u128>,
) -> Line<'static> {
    crate::runtime::status::status_test::compact_status_line(
        active_model_identity,
        None,
        now_millis,
        120,
        &crate::rendering::theme::TuiTheme::default(),
    )
}

pub(crate) fn lane_2_status_line_for_test(state: &AppState, width: usize) -> Line<'static> {
    crate::runtime::status::status_test::lane_2_status_line(
        state,
        width,
        &crate::rendering::theme::TuiTheme::default(),
    )
}

pub(crate) fn status_left_content_for_test(
    model: &str,
    busy_millis: Option<u128>,
    state: &AppState,
    width: usize,
) -> Line<'static> {
    crate::runtime::status::status_left_content(
        model,
        busy_millis,
        state,
        &crate::rendering::theme::TuiTheme::default(),
        width,
    )
}

pub(crate) fn status_right_content_for_test(
    repo_branch: Option<&str>,
    cwd: Option<&std::path::Path>,
) -> Option<Line<'static>> {
    crate::runtime::status::status_right_content(
        repo_branch,
        cwd,
        &crate::rendering::theme::TuiTheme::default(),
    )
}

pub(crate) fn status_lines_for_test(state: &AppState, active_model_identity: &str) -> Vec<String> {
    crate::runtime::status::build_status_lines(state, active_model_identity)
}

pub(crate) fn cursor_style_for_test(
    mode: crate::state::InputMode,
) -> crossterm::cursor::SetCursorStyle {
    mode.cursor_style()
}

pub(crate) fn status_indicator_for_test(now_millis: Option<u128>) -> &'static str {
    crate::runtime::status::status_test::status_indicator_for_test(now_millis)
}

pub(crate) fn transition_spacer_for_roles_for_test(
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

pub(crate) fn input_line_for_test(state: &AppState) -> String {
    let _ = state;
    let _ = crate::runtime::render_frame::current_time_millis();
    String::new()
}

pub(crate) fn input_line_for_test_at_millis(state: &AppState, now_millis: u128) -> String {
    let _ = state;
    let _ = now_millis;
    String::new()
}

pub(crate) fn input_prompt_prefix(mode: crate::state::InputMode) -> &'static str {
    match mode {
        crate::state::InputMode::Insert => "❯ ",
        crate::state::InputMode::Normal | crate::state::InputMode::Visual => "❮ ",
    }
}

pub(crate) fn input_rows_with_prompt_for_test(state: &AppState, pane_width: u16) -> Vec<String> {
    let rows =
        crate::rendering::layout::wrapped_input_rows("", pane_width.saturating_sub(2) as usize);

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

pub(crate) fn model_picker_row_cells_for_test(state: &AppState) -> Vec<Vec<String>> {
    state
        .model_picker_filtered_options()
        .iter()
        .map(|option| {
            let active = if option.active { "*" } else { "" };
            vec![option.identity.clone(), active.to_string()]
        })
        .collect()
}

pub(crate) fn agent_picker_row_cells_for_test(state: &AppState) -> Vec<Vec<String>> {
    state
        .agent_picker_filtered_options()
        .iter()
        .map(|option| {
            let active = if option.active { "*" } else { "" };
            let desc = option.description.as_deref().unwrap_or("");
            vec![option.name.clone(), desc.to_string(), active.to_string()]
        })
        .collect()
}
