use ratatui::text::Line;

use unicode_width::UnicodeWidthStr;

use crate::state::{AppState, PickerPayload};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The cue shape the overflow path builds: the keys hint plus the position
/// tail, which pushes the Status title past its title row at the popup's
/// minimum width.
const SCROLL_PANEL_OVERFLOW_CUE: &str = "↑/↓ · PgUp/PgDn · Esc · 1-6 / 128";

/// The keys-only cue every scroll panel shows while its content fits.
const SCROLL_PANEL_KEYS_CUE: &str = "↑/↓ · PgUp/PgDn · Esc";

pub(crate) fn help_panel_max_scroll_for_test(lines: &[Line<'_>], viewport_height: u16) -> usize {
    crate::runtime::panels::help_panel_max_scroll(lines, viewport_height)
}

pub(crate) fn help_panel_overflow_cue_for_test(
    lines: &[Line<'_>],
    viewport_height: u16,
    scroll: usize,
) -> String {
    crate::runtime::panels::help_panel_overflow_cue(lines, viewport_height, scroll)
}

pub(crate) fn help_panel_visible_window_for_test<'a>(
    lines: &[Line<'a>],
    viewport_height: u16,
    scroll: usize,
) -> Vec<Line<'a>> {
    crate::runtime::panels_test::help_panel_visible_window(
        lines,
        scroll,
        viewport_height.max(1) as usize,
    )
}

/// Returns the effective (clamped) scroll offset that would be passed to
/// `render_scroll_text_panel` for the Help panel given a viewport inner size and
/// a requested scroll offset. The clamp uses `help_panel_max_scroll`, which
/// counts the lines the panel builder wrapped at the viewport width — the same
/// rows the render paints.
///
/// The Status and Skills panels share the clamp and the overflow cue: their
/// lines are wrapped at the same viewport width, so their titles report true
/// ranges.
pub(crate) fn help_panel_scroll_offset_for_test(
    viewport_height: u16,
    viewport_width: u16,
    requested_scroll: usize,
) -> usize {
    let (_title, lines) = crate::runtime::status::help::help_panel_lines(
        &crate::rendering::theme::TuiTheme::default(),
        viewport_width,
    );
    requested_scroll.min(crate::runtime::panels::help_panel_max_scroll(
        &lines,
        viewport_height,
    ))
}

pub(crate) fn status_panel_scroll_offset_for_test(
    state: &AppState,
    viewport_height: u16,
    viewport_width: u16,
    requested_scroll: usize,
) -> usize {
    let (_title, lines) = crate::runtime::status::help::status_panel_lines(state, viewport_width);
    requested_scroll.min(crate::runtime::panels::help_panel_max_scroll(
        &lines,
        viewport_height,
    ))
}

pub(crate) fn skills_panel_scroll_offset_for_test(
    state: &AppState,
    viewport_height: u16,
    viewport_width: u16,
    requested_scroll: usize,
) -> usize {
    let (_title, lines) = crate::runtime::panels::skills_panel_lines(state, viewport_width);
    requested_scroll.min(crate::runtime::panels::help_panel_max_scroll(
        &lines,
        viewport_height,
    ))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct McpTableModelForTest {
    pub controls_line: String,
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
    table_width: u16,
    table_height: u16,
) -> McpTableModelForTest {
    let model = crate::runtime::panels::mcp_table_model(state, table_width, table_height);
    McpTableModelForTest {
        controls_line: model.controls_line,
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
    table_width: u16,
    popup_height: u16,
) -> CommandPaletteTableModelForTest {
    let model =
        crate::runtime::panels::command_palette_table_model(state, table_width, popup_height);
    CommandPaletteTableModelForTest {
        query_line: model.query_line,
        columns: model.columns,
        rows: model.rows,
        selected: model.selected,
        overflow_cue: model.overflow_cue,
    }
}

pub(crate) fn inline_slash_lines_for_test(state: &AppState) -> Vec<String> {
    crate::runtime::status::help::inline_slash_lines_for_render(
        state,
        &crate::rendering::theme::TuiTheme::default(),
    )
    .into_iter()
    .map(|line| line.to_string())
    .collect()
}

pub(crate) fn compact_status_line_for_test(
    active_model_identity: &str,
    now_millis: Option<u128>,
) -> Line<'static> {
    crate::runtime::status::test::compact_status_line(
        active_model_identity,
        None,
        now_millis,
        120,
        &crate::rendering::theme::TuiTheme::default(),
    )
}

pub(crate) fn lane_2_status_line_for_test(state: &AppState, width: usize) -> Line<'static> {
    crate::runtime::status::test::lane_2_status_line(
        state,
        width,
        &crate::rendering::theme::TuiTheme::default(),
    )
}

pub(crate) fn status_left_content_for_test(
    model: &str,
    busy_millis: Option<u128>,
    state: &mut AppState,
    width: usize,
) -> Line<'static> {
    state.status.identity.active_model_identity = model.to_string();
    crate::runtime::status::status_left_content(
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

pub(crate) fn status_lines_for_test(
    state: &mut AppState,
    active_model_identity: &str,
) -> Vec<String> {
    state.status.identity.active_model_identity = active_model_identity.to_string();
    crate::runtime::status::build_status_lines(state)
}

pub(crate) fn cursor_style_for_test(
    mode: crate::state::InputMode,
) -> crossterm::cursor::SetCursorStyle {
    mode.cursor_style()
}

pub(crate) fn status_indicator_for_test(now_millis: Option<u128>) -> &'static str {
    crate::runtime::status::test::status_indicator_for_test(now_millis)
}

pub(crate) fn input_line_for_test(state: &AppState) -> String {
    let _ = state;
    let _ = crate::runtime::render::frame::current_time_millis();
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
    let prompt_prefix = input_prompt_prefix(state.input.mode);
    if let Some((first, rest)) = rows.split_first() {
        lines.push(format!("{prompt_prefix}{first}"));
        for row in rest {
            lines.push(format!("  {row}"));
        }
    }

    lines
}

pub(crate) fn model_picker_row_cells_for_test(state: &AppState) -> Vec<Vec<String>> {
    let Some(picker_state) = state.picker.active_state() else {
        return Vec::new();
    };
    picker_state
        .filtered()
        .iter()
        .map(|opt| {
            let (identity, configured, context_window, cost) = match &opt.payload {
                PickerPayload::Model {
                    identity,
                    configured,
                    context_window,
                    cost,
                    ..
                } => (identity.clone(), *configured, *context_window, cost.clone()),
                _ => (String::new(), false, None, None),
            };
            let active = if opt.id == state.status.identity.active_model_identity {
                "*"
            } else {
                ""
            };
            let configured = if configured { "◆" } else { "" };
            let context_cell = context_window
                .map(|n| format!("{}k", n / 1000))
                .unwrap_or_default();
            vec![
                identity,
                context_cell,
                cost.unwrap_or_default(),
                active.to_string(),
                configured.to_string(),
            ]
        })
        .collect()
}

pub(crate) fn agent_picker_row_cells_for_test(state: &AppState) -> Vec<Vec<String>> {
    let Some(picker_state) = state.picker.active_state() else {
        return Vec::new();
    };
    picker_state
        .filtered()
        .iter()
        .map(|opt| {
            let (name, description) = match &opt.payload {
                PickerPayload::Agent { name, description } => (name.clone(), description.clone()),
                _ => (String::new(), None),
            };
            let active = if Some(opt.id.as_str())
                == state.status.identity.active_agent_identity.as_deref()
            {
                "*"
            } else {
                ""
            };
            vec![name, description.unwrap_or_default(), active.to_string()]
        })
        .collect()
}

/// At the Status popup's minimum width the position tail pushes the composed
/// title past the title row, so the title ellipsizes to that row with a
/// trailing marker instead of hard-clipping at the frame border.
#[test]
fn test_panels_scroll_panel_title_ellipsizes_overflow_cue() -> Result<()> {
    // -- Setup & Fixtures
    let unbounded = format!("Status ({SCROLL_PANEL_OVERFLOW_CUE})");
    let row_width = 40usize;

    // -- Exec
    let title = crate::runtime::panels::scroll_panel_title("Status", SCROLL_PANEL_OVERFLOW_CUE, 42);

    // -- Check
    assert!(
        UnicodeWidthStr::width(unbounded.as_str()) > row_width,
        "the fixture must overflow the {row_width}-cell title row; unbounded: {unbounded:?}"
    );
    assert_eq!(
        UnicodeWidthStr::width(title.as_str()),
        row_width,
        "the title must fill the {row_width}-cell title row; title: {title:?}"
    );
    assert!(
        title.ends_with('…'),
        "the overflowing title must ellipsize with a marker; title: {title:?}"
    );
    Ok(())
}

/// The keys-only Status title fits the title row at the popup's minimum width,
/// so it renders verbatim with no marker.
#[test]
fn test_panels_scroll_panel_title_renders_fitting_cue_verbatim() -> Result<()> {
    // -- Setup & Fixtures
    let expected = format!("Status ({SCROLL_PANEL_KEYS_CUE})");
    let row_width = 40usize;

    // -- Exec
    let title = crate::runtime::panels::scroll_panel_title("Status", SCROLL_PANEL_KEYS_CUE, 42);

    // -- Check
    assert!(
        UnicodeWidthStr::width(expected.as_str()) <= row_width,
        "the fixture must fit the {row_width}-cell title row; expected: {expected:?}"
    );
    assert_eq!(
        title, expected,
        "a fitting title must render verbatim with no marker"
    );
    Ok(())
}

/// The title never exceeds the title row across the Status popup's whole width
/// range: it truncates to the row while it overflows and renders whole once the
/// overflow cue fits.
#[test]
fn test_panels_scroll_panel_title_fits_title_row_at_every_popup_width() -> Result<()> {
    // -- Setup & Fixtures: the unbounded title the renderer would clip.
    let unbounded = format!("Status ({SCROLL_PANEL_OVERFLOW_CUE})");
    let unbounded_width = UnicodeWidthStr::width(unbounded.as_str());

    // -- Exec & Check: popup widths 42..=72 span the Status popup's range.
    for popup_width in 42u16..=72 {
        let title = crate::runtime::panels::scroll_panel_title(
            "Status",
            SCROLL_PANEL_OVERFLOW_CUE,
            popup_width,
        );
        let row_width = usize::from(popup_width.saturating_sub(2));
        assert_eq!(
            UnicodeWidthStr::width(title.as_str()),
            unbounded_width.min(row_width),
            "the title must fill the {row_width}-cell title row at popup width \
             {popup_width}; title: {title:?}"
        );
        if unbounded_width > row_width {
            assert!(
                title.ends_with('…'),
                "the overflowing title must truncate at popup width {popup_width}; \
                 title: {title:?}"
            );
        } else {
            assert_eq!(
                title, unbounded,
                "the fitting title must render verbatim at popup width {popup_width}"
            );
        }
    }
    Ok(())
}
