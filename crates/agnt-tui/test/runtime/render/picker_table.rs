use ratatui::{
    Terminal,
    backend::TestBackend,
    layout::{Constraint, Rect},
    widgets::Row,
};

use crate::{
    rendering::theme::TuiTheme,
    runtime::{
        panels::PICKER_HIGHLIGHT_SYMBOL,
        render::picker_table::{PickerTableLayout, render_picker_table, section_header_row},
    },
};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The table owns the header style: the caller hands over unstyled header cells
/// and the rendered header carries the popup's meta style, with the rows below
/// it in the caller's order.
#[test]
fn test_runtime_picker_table_paints_meta_styled_header() -> Result<()> {
    // -- Setup & Fixtures
    const WIDTH: u16 = 24;
    let theme = TuiTheme::default();
    let header = Row::new(vec!["NAME", "N"]);
    let rows = vec![Row::new(vec!["alpha", "1"]), Row::new(vec!["beta", "2"])];
    let columns = [Constraint::Length(8), Constraint::Length(2)];
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 4))?;

    // -- Exec
    terminal.draw(|frame| {
        render_picker_table(
            frame,
            Rect::new(0, 0, WIDTH, 4),
            header,
            rows,
            PickerTableLayout {
                columns: &columns,
                spacing: 1,
            },
            None,
            &theme,
        );
    })?;
    let buffer = terminal.backend().buffer();
    let header_text = (0..WIDTH)
        .map(|x| buffer[(x, 0)].symbol())
        .collect::<String>();
    let body_text = (0..WIDTH)
        .map(|x| buffer[(x, 1)].symbol())
        .collect::<String>();
    let header_at = header_text
        .find("NAME")
        .ok_or("the header row should carry the caller's label")?;
    let header_x = u16::try_from(header_at)?;
    let header_style = buffer[(header_x, 0)].style();

    // -- Check
    assert_eq!(
        header_style.fg, theme.subtle_meta.fg,
        "the header must carry the popup's meta foreground; style: {header_style:?}"
    );
    assert_eq!(
        header_style.add_modifier, theme.subtle_meta.add_modifier,
        "the header must carry the meta style's modifiers; style: {header_style:?}"
    );
    assert!(
        body_text.contains("alpha"),
        "the first caller row must render below the header; rendered: {body_text:?}"
    );
    Ok(())
}

/// The selection symbol and the focus style land on the selected row only, and
/// the header and every other row stay unfocused.
#[test]
fn test_runtime_picker_table_highlights_the_selected_row() -> Result<()> {
    // -- Setup & Fixtures
    const WIDTH: u16 = 24;
    let theme = TuiTheme::default();
    let header = Row::new(vec!["NAME", "N"]);
    let rows = vec![Row::new(vec!["alpha", "1"]), Row::new(vec!["beta", "2"])];
    let columns = [Constraint::Length(8), Constraint::Length(2)];
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 4))?;

    // -- Exec
    terminal.draw(|frame| {
        render_picker_table(
            frame,
            Rect::new(0, 0, WIDTH, 4),
            header,
            rows,
            PickerTableLayout {
                columns: &columns,
                spacing: 1,
            },
            Some(1),
            &theme,
        );
    })?;
    let buffer = terminal.backend().buffer();
    let row_text = |y: u16| {
        (0..WIDTH)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    };
    let first_row = row_text(1);
    let second_row = row_text(2);
    let symbol_at = second_row
        .find(PICKER_HIGHLIGHT_SYMBOL.trim_end())
        .ok_or("the selected row should carry the selection symbol")?;
    let focused_style = buffer[(u16::try_from(symbol_at)?, 2)].style();
    let alpha_at = first_row
        .find("alpha")
        .ok_or("the unselected row should still render its cells")?;
    let unfocused_style = buffer[(u16::try_from(alpha_at)?, 1)].style();

    // -- Check
    assert!(
        !first_row.contains(PICKER_HIGHLIGHT_SYMBOL.trim_end()),
        "an unselected row must not carry the selection symbol; rendered: {first_row:?}"
    );
    assert_eq!(
        focused_style.add_modifier, theme.focus.add_modifier,
        "the selected row must carry the focus modifiers; style: {focused_style:?}"
    );
    assert_eq!(
        focused_style.fg, theme.focus.fg,
        "the selected row must carry the focus foreground; style: {focused_style:?}"
    );
    assert!(
        unfocused_style.add_modifier != theme.focus.add_modifier
            || unfocused_style.fg != theme.focus.fg,
        "an unselected row must not carry the focus style; style: {unfocused_style:?}"
    );
    Ok(())
}

/// With no selection the table paints no selection symbol at all.
#[test]
fn test_runtime_picker_table_paints_no_symbol_without_selection() -> Result<()> {
    // -- Setup & Fixtures
    const WIDTH: u16 = 24;
    let theme = TuiTheme::default();
    let rows = vec![Row::new(vec!["alpha", "1"])];
    let columns = [Constraint::Length(8), Constraint::Length(2)];
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 3))?;

    // -- Exec
    terminal.draw(|frame| {
        render_picker_table(
            frame,
            Rect::new(0, 0, WIDTH, 3),
            Row::new(vec!["NAME", "N"]),
            rows,
            PickerTableLayout {
                columns: &columns,
                spacing: 1,
            },
            None,
            &theme,
        );
    })?;
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    // -- Check
    assert!(
        !rendered.contains(PICKER_HIGHLIGHT_SYMBOL.trim_end()),
        "an unselected table must paint no selection symbol; rendered: {rendered:?}"
    );
    Ok(())
}

/// A section header row carries the label verbatim in the section-header style:
/// the caller supplies the indent, so a bare label starts in the row's first
/// cell and a prefixed label starts after its prefix.
#[test]
fn test_runtime_picker_table_section_header_keeps_the_caller_indent() -> Result<()> {
    // -- Setup & Fixtures
    const WIDTH: u16 = 24;
    let theme = TuiTheme::default();
    let bare = "Ollama";
    let indented = "  OpenAI";
    let rows = vec![
        section_header_row(bare, &theme),
        Row::new(vec!["ollama/llama3"]),
        section_header_row(indented, &theme),
    ];
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 5))?;

    // -- Exec
    terminal.draw(|frame| {
        render_picker_table(
            frame,
            Rect::new(0, 0, WIDTH, 5),
            Row::new(vec!["MODEL"]),
            rows,
            PickerTableLayout {
                columns: &[Constraint::Min(10)],
                spacing: 1,
            },
            None,
            &theme,
        );
    })?;
    let buffer = terminal.backend().buffer();
    let row_text = |y: u16| {
        (0..WIDTH)
            .map(|x| buffer[(x, y)].symbol())
            .collect::<String>()
    };
    let bare_row = row_text(1);
    let data_row = row_text(2);
    let indented_row = row_text(3);
    let bare_style = buffer[(0, 1)].style();
    let data_style = buffer[(0, 2)].style();
    let indented_style = buffer[(2, 3)].style();

    // -- Check
    assert!(
        bare_row.trim_start().starts_with(bare),
        "a bare label must render in the row; rendered: {bare_row:?}"
    );
    assert!(
        indented_row.starts_with("  OpenA"),
        "a prefixed label must keep its own indent; rendered: {indented_row:?}"
    );
    assert!(
        bare_row.find(bare) == data_row.find("ollama/llama3"),
        "the section label and a data cell must start on the same column;\nsection: {bare_row:?}\ndata: {data_row:?}"
    );
    assert_eq!(
        (bare_style.fg, bare_style.add_modifier),
        (
            theme.picker_section_header.fg,
            theme.picker_section_header.add_modifier
        ),
        "the label must carry the section-header style; style: {bare_style:?}"
    );
    assert_eq!(
        (indented_style.fg, indented_style.add_modifier),
        (
            theme.picker_section_header.fg,
            theme.picker_section_header.add_modifier
        ),
        "the prefixed label must carry the section-header style; style: {indented_style:?}"
    );
    assert!(
        data_style.fg != theme.picker_section_header.fg
            || data_style.add_modifier != theme.picker_section_header.add_modifier,
        "a data row must not carry the section-header style; style: {data_style:?}"
    );
    Ok(())
}
