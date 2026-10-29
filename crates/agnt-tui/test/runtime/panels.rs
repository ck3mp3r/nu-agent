use ratatui::{Terminal, backend::TestBackend, layout::Rect, text::Line};
use unicode_width::UnicodeWidthStr;

use crate::{
    rendering::theme::TuiTheme,
    runtime::panels::{
        mcp_details_rows, picker_details_height_for_inner_height, popup_title,
        render_permission_controls,
    },
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// `picker_details_height_for_inner_height` follows the step table for inner
/// heights 0..=20: no footer row up to inner 7, two rows from 8 to 12, three
/// rows above.
#[test]
fn test_runtime_panels_picker_details_height_step_table() -> Result<()> {
    // -- Setup & Fixtures
    let expected: [(u16, u16); 21] = [
        (0, 0),
        (1, 0),
        (2, 0),
        (3, 0),
        (4, 0),
        (5, 0),
        (6, 0),
        (7, 0),
        (8, 2),
        (9, 2),
        (10, 2),
        (11, 2),
        (12, 2),
        (13, 3),
        (14, 3),
        (15, 3),
        (16, 3),
        (17, 3),
        (18, 3),
        (19, 3),
        (20, 3),
    ];

    // -- Exec & Check
    for (h, want) in expected {
        assert_eq!(
            picker_details_height_for_inner_height(h),
            want,
            "inner_height={h}"
        );
    }
    Ok(())
}

/// The MCP details rows are exactly the rows the pane paints: a fitting line
/// renders verbatim, an over-wide line wraps into the width left after its
/// indent with that indent on every row, and the stack is cut at the ladder
/// cap.
#[test]
fn test_runtime_panels_mcp_details_rows_wrap_and_cap() -> Result<()> {
    // -- Setup & Fixtures
    const INDENT: &str = "       ";
    let body = "gh__averylongtoolnamethatdoesnotfitatallinthepane_xxxx";
    let lines = vec![
        Line::from("Server: gh (enabled)"),
        Line::from(format!("{INDENT}{body}")),
        Line::from("Tools: dropped"),
    ];

    // -- Exec
    let rows = mcp_details_rows(&lines, 30, 6);
    let capped = mcp_details_rows(&lines, 30, 2);

    // -- Check: the fitting rows render verbatim, above and below the wrapped
    // tool row.
    assert_eq!(rows[0], Line::from("Server: gh (enabled)"));
    assert_eq!(rows[rows.len() - 1], Line::from("Tools: dropped"));
    assert_eq!(rows.len(), 5, "rows: {rows:?}");

    // -- Check: every wrapped row keeps the indent and together they carry the
    // whole body at width 30.
    let wrapped = &rows[1..rows.len() - 1];
    assert!(
        wrapped
            .iter()
            .all(|row| row.to_string().starts_with(INDENT)),
        "every wrapped row must keep the indent; rows: {wrapped:?}"
    );
    let body_chars: usize = wrapped
        .iter()
        .map(|row| row.to_string().chars().count() - INDENT.chars().count())
        .sum();
    assert_eq!(
        body_chars,
        body.chars().count(),
        "the wrapped rows must carry every body character; rows: {wrapped:?}"
    );

    // -- Check: the ladder cap cuts the stack.
    assert_eq!(
        capped.len(),
        2,
        "the cap must cut the stack; rows: {capped:?}"
    );
    assert!(
        mcp_details_rows(&lines, 30, 0).is_empty(),
        "no cap rows must claim no rows"
    );
    Ok(())
}

#[test]
fn test_runtime_panels_permission_controls_deny_key_only() -> Result<()> {
    // -- Setup & Fixtures
    const WIDTH: u16 = 60;
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 1))?;
    let theme = TuiTheme::default();

    // -- Exec
    terminal.draw(|frame| {
        render_permission_controls(frame, Rect::new(0, 0, WIDTH, 1), &theme);
    })?;

    // -- Check
    let buffer = terminal.backend().buffer();
    let rendered = (0..WIDTH)
        .map(|x| buffer[(x, 0)].symbol())
        .collect::<String>();
    assert!(
        rendered.contains("[d] deny"),
        "footer must show [d] deny, got: {rendered}"
    );
    assert!(
        !rendered.contains("Esc"),
        "footer must not mention Esc, got: {rendered}"
    );
    Ok(())
}

/// `popup_title` joins the base with its optional keys and cue: keys and cue
/// share one parenthesis pair, keys alone keep their own, a cue alone renders
/// bare, and no hint leaves the bare base.
#[test]
fn test_runtime_panels_popup_title_joins_keys_and_cue_shapes() -> Result<()> {
    // -- Setup & Fixtures
    const KEYS: &str = "↑/↓ · Enter · Esc";
    const CUE: &str = "1-5 / 8";

    // -- Exec
    let keys_and_cue = popup_title("Sessions", Some(KEYS), Some(CUE), 90);
    let keys_only = popup_title("Models", Some(KEYS), None, 90);
    let cue_only = popup_title("MCPs", None, Some(CUE), 90);
    let bare = popup_title("MCPs", None, None, 90);

    // -- Check
    assert_eq!(keys_and_cue, "Sessions (↑/↓ · Enter · Esc · 1-5 / 8)");
    assert_eq!(keys_only, "Models (↑/↓ · Enter · Esc)");
    assert_eq!(cue_only, "MCPs (1-5 / 8)");
    assert_eq!(bare, "MCPs");
    Ok(())
}

/// A title past the popup's title row ellipsizes to that row with a trailing
/// marker, and a fitting title renders whole: the title never exceeds the
/// `popup_width - 2` cells the row holds.
#[test]
fn test_runtime_panels_popup_title_ellipsizes_to_title_row() -> Result<()> {
    // -- Setup & Fixtures: the longest real picker title.
    const KEYS: &str = "↑/↓ · Enter · Esc";
    const CUE: &str = "999 of 9999";
    let unbounded = format!("Sessions ({KEYS} · {CUE})");
    let unbounded_width = UnicodeWidthStr::width(unbounded.as_str());

    // -- Exec & Check: popup widths 28..=90 span every picker's range.
    for popup_width in 28u16..=90 {
        let title = popup_title("Sessions", Some(KEYS), Some(CUE), popup_width);
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
                "the overflowing title must ellipsize with a marker at popup width \
                 {popup_width}; title: {title:?}"
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

/// The rows a scroll panel paints at `scroll`: the panel builders pre-wrap
/// their lines to the panel width, so the visible window is a slice of those
/// lines.
pub(crate) fn help_panel_visible_window<'a>(
    lines: &[Line<'a>],
    scroll: usize,
    rows: usize,
) -> Vec<Line<'a>> {
    lines.iter().skip(scroll).take(rows).cloned().collect()
}
