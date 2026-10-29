use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Modifier};

use crate::{
    rendering::theme::TuiTheme,
    runtime::{panels::THEME_PICKER_EMPTY_STATE_MESSAGE, render::render_empty_state},
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The empty state paints the message on the rect's first row, in the popup's
/// meta foreground with no background and no bold.
#[test]
fn test_runtime_render_empty_state_paints_meta_text() -> Result<()> {
    // -- Setup & Fixtures
    const WIDTH: u16 = 40;
    let message = THEME_PICKER_EMPTY_STATE_MESSAGE;
    let theme = TuiTheme::default();
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 3))?;

    // -- Exec
    terminal.draw(|frame| {
        render_empty_state(frame, Rect::new(0, 0, WIDTH, 2), message, &theme);
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
        first_row.starts_with(message),
        "the message must start at the rect's first row; rendered: {first_row:?}"
    );
    assert_eq!(
        cell_style.fg, theme.tool_meta.fg,
        "the message must carry the popup's meta foreground; style: {cell_style:?}"
    );
    assert_eq!(
        cell_style.bg, bare_style.bg,
        "the message must carry no background; style: {cell_style:?}"
    );
    assert!(
        !cell_style.add_modifier.contains(Modifier::BOLD),
        "the message must not be bold; style: {cell_style:?}"
    );
    Ok(())
}

/// The empty state writes only inside the rect it is given: a rect that starts
/// below the buffer's first row and to the right of its first column leaves
/// every cell around it untouched.
#[test]
fn test_runtime_render_empty_state_stays_inside_its_rect() -> Result<()> {
    // -- Setup & Fixtures
    const WIDTH: u16 = 30;
    const X: u16 = 4;
    let message = "No themes available";
    let theme = TuiTheme::default();
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 3))?;

    // -- Exec
    terminal.draw(|frame| {
        render_empty_state(frame, Rect::new(X, 1, 20, 1), message, &theme);
    })?;
    let buffer = terminal.backend().buffer();
    let above = (0..WIDTH)
        .map(|x| buffer[(x, 0)].symbol())
        .collect::<String>();
    let first_glyph = buffer[(X, 1)].symbol();
    let before_rect = buffer[(X - 1, 1)].symbol();
    let after_text = buffer[(X + 19, 1)].symbol();

    // -- Check
    assert_eq!(
        above.trim(),
        "",
        "the row above the rect must stay unpainted; rendered: {above:?}"
    );
    assert_eq!(
        first_glyph, "N",
        "the message must start at the rect's left edge; symbol: {first_glyph:?}"
    );
    assert_eq!(
        before_rect, " ",
        "the cell before the rect must stay unpainted; symbol: {before_rect:?}"
    );
    assert_eq!(
        after_text, " ",
        "the message must end inside the rect's width; symbol: {after_text:?}"
    );
    Ok(())
}
