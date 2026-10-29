use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Modifier, text::Line};

use crate::{
    rendering::theme::TuiTheme,
    runtime::{panels::picker_query_line, render::render_popup_hint_row},
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The hint row paints its line on the rect's first row in the popup's meta
/// style, with no background and no bold of its own.
#[test]
fn test_runtime_render_popup_hint_row_paints_meta_line() -> Result<()> {
    // -- Setup & Fixtures
    const WIDTH: u16 = 30;
    const LINE: &str = "↑/↓ select · Enter toggle";
    let theme = TuiTheme::default();
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 3))?;

    // -- Exec
    terminal.draw(|frame| {
        render_popup_hint_row(frame, Rect::new(0, 0, WIDTH, 2), Line::from(LINE), &theme);
    })?;
    let buffer = terminal.backend().buffer();
    let first_row = (0..WIDTH)
        .map(|x| buffer[(x, 0)].symbol())
        .collect::<String>();
    let cell_style = buffer[(0, 0)].style();
    // Row 2 sits outside the rect, so its style is the reference for "no
    // background".
    let bare_style = buffer[(0, 2)].style();

    // -- Check
    assert!(
        first_row.starts_with(LINE),
        "the line must start at the rect's first row; rendered: {first_row:?}"
    );
    assert_eq!(
        cell_style.fg, theme.subtle_meta.fg,
        "the line must carry the popup's meta foreground; style: {cell_style:?}"
    );
    assert_eq!(
        cell_style.bg, bare_style.bg,
        "the line must carry no background; style: {cell_style:?}"
    );
    assert!(
        !cell_style.add_modifier.contains(Modifier::BOLD),
        "the line must not be bold; style: {cell_style:?}"
    );
    Ok(())
}

/// The hint row leaves the rows below its one-row rect untouched, so the table
/// underneath keeps its own surface.
#[test]
fn test_runtime_render_popup_hint_row_stays_in_its_row() -> Result<()> {
    // -- Setup & Fixtures
    const WIDTH: u16 = 30;
    let theme = TuiTheme::default();
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 4))?;

    // -- Exec
    terminal.draw(|frame| {
        render_popup_hint_row(
            frame,
            Rect::new(0, 1, WIDTH, 1),
            Line::from("Select a color theme"),
            &theme,
        );
    })?;
    let buffer = terminal.backend().buffer();
    let above = (0..WIDTH)
        .map(|x| buffer[(x, 0)].symbol())
        .collect::<String>();
    let below = (0..WIDTH)
        .map(|x| buffer[(x, 2)].symbol())
        .collect::<String>();
    let hint_row = (0..WIDTH)
        .map(|x| buffer[(x, 1)].symbol())
        .collect::<String>();

    // -- Check
    assert_eq!(
        above.trim(),
        "",
        "the row above the hint must stay unpainted; rendered: {above:?}"
    );
    assert_eq!(
        below.trim(),
        "",
        "the row below the hint must stay unpainted; rendered: {below:?}"
    );
    assert!(
        hint_row.starts_with("Select a color theme"),
        "the line must paint only the hint row; rendered: {hint_row:?}"
    );
    Ok(())
}

/// The query line helper is the only place the prefix is formatted: it puts the
/// prefix in front of the query and leaves an empty query showing the bare
/// prefix.
#[test]
fn test_runtime_picker_query_line_prefixes_the_query() -> Result<()> {
    // -- Exec & Check
    assert_eq!(
        picker_query_line("gpt"),
        "Query: gpt",
        "the helper must put the prefix in front of the query"
    );
    assert_eq!(
        picker_query_line(""),
        "Query: ",
        "an empty query must still show the bare prefix"
    );
    Ok(())
}
