use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Modifier};

use crate::{rendering::theme::TuiTheme, runtime::render::PickerFooter};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// The measured height is the wrapped row count plus the gap row, capped at the
/// ladder budget.
#[test]
fn test_runtime_picker_footer_measure_sizes_to_wrapped_text() -> Result<()> {
    // -- Setup & Fixtures
    // The selected model identity at a 60-wide popup (inner width 58) on a
    // 4-cell text indent, so the text wraps at 53.
    let identity = "openrouter/anthropic/claude-opus-4.5";
    // A single 71-cell word: no space to break on, so it fills the 53-cell wrap
    // column and carries the remaining 18 cells onto a second row.
    let unbroken = "a".repeat(71);
    // A single 56-cell word fits the inner width but overflows the wrap column,
    // so it takes two rows.
    let over_wrap = "b".repeat(56);
    // Nine 4-cell words at width 20 (wrap column 15): three per row, so three
    // rows.
    let three_rows = ["dddd"; 9].join(" ");

    // -- Exec & Check
    assert_eq!(
        PickerFooter::measure(Some(identity), 4, 58, 3).height(),
        2,
        "a one-row footer must claim two of the three reserved rows: the gap and the text"
    );
    assert_eq!(
        PickerFooter::measure(Some(&unbroken), 4, 58, 2).height(),
        3,
        "a 71-cell word must fill both reserved text rows and add the gap row"
    );
    assert_eq!(
        PickerFooter::measure(Some(&over_wrap), 4, 58, 2).height(),
        3,
        "the text must wrap five columns short of the inner width, so 56 cells take two rows"
    );
    assert_eq!(
        PickerFooter::measure(Some(&three_rows), 4, 20, 2).height(),
        3,
        "a three-row footer must be capped at the two reserved rows plus the gap row"
    );
    assert_eq!(
        PickerFooter::measure(Some(identity), 4, 58, 0).height(),
        0,
        "no reserved rows must claim no rows"
    );
    assert_eq!(
        PickerFooter::measure(None, 4, 58, 3).height(),
        0,
        "no text must claim no rows"
    );
    Ok(())
}

/// The rendered rows carry the measured rows: every character survives across
/// the wrapped text rows, and the cap drops the tail instead of truncating with
/// a marker.
#[test]
fn test_runtime_picker_footer_render_carries_every_character() -> Result<()> {
    // -- Setup & Fixtures
    const WIDTH: u16 = 58;
    let unbroken = "a".repeat(71);
    let theme = TuiTheme::default();
    let footer = PickerFooter::measure(Some(&unbroken), 4, WIDTH, 2);
    let capped = PickerFooter::measure(Some(&unbroken), 4, WIDTH, 1);
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, footer.height()))?;

    // -- Exec
    terminal.draw(|frame| {
        footer.render(frame, Rect::new(0, 0, WIDTH, footer.height()), &theme);
    })?;
    let rendered = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    // -- Check
    assert_eq!(
        footer.height(),
        3,
        "the 71-cell word must fill two text rows plus the gap row"
    );
    assert_eq!(
        rendered.chars().filter(|c| *c == 'a').count(),
        71,
        "two rows at the cap must carry every character; rendered: {rendered:?}"
    );
    assert!(
        !rendered.contains('…'),
        "the footer must not truncate with a marker; rendered: {rendered:?}"
    );
    assert_eq!(
        capped.height(),
        2,
        "the cap must drop the tail instead of growing the text rows"
    );
    Ok(())
}

/// The footer paints foreground-only text below the gap row, starting at the
/// surface's text column: the text carries no background, no bold, and no cell
/// in the footer rect carries a filled background behind it.
#[test]
fn test_runtime_picker_footer_render_paints_fg_only_text() -> Result<()> {
    // -- Setup & Fixtures
    const WIDTH: u16 = 58;
    const TEXT_INDENT: u16 = 4;
    let identity = "openrouter/anthropic/claude-opus-4.5";
    let theme = TuiTheme::default();
    let footer = PickerFooter::measure(Some(identity), TEXT_INDENT, WIDTH, 2);
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, footer.height()))?;

    // -- Exec
    terminal.draw(|frame| {
        footer.render(frame, Rect::new(0, 0, WIDTH, footer.height()), &theme);
    })?;
    let buffer = terminal.backend().buffer();
    // The last cell of the gap row is never painted, so its style is the
    // reference for "no background".
    let bare_style = buffer[(WIDTH - 1, 0)].style();
    let gap_symbol = buffer[(TEXT_INDENT, 0)].symbol().to_string();
    let gap_style = buffer[(TEXT_INDENT, 0)].style();
    let leading_style = buffer[(0, 1)].style();
    let text_symbol = buffer[(TEXT_INDENT, 1)].symbol().to_string();
    let text_style = buffer[(TEXT_INDENT, 1)].style();
    let trailing_style = buffer[(WIDTH - 1, 1)].style();

    // -- Check
    assert_eq!(
        gap_symbol, " ",
        "the gap row must carry no text; symbol: {gap_symbol:?}"
    );
    assert_eq!(
        text_symbol, "o",
        "the first text glyph must start at the text indent; symbol: {text_symbol:?}"
    );
    assert_eq!(
        text_style.fg, theme.subtle_meta.fg,
        "the text must carry the popup's meta foreground; style: {text_style:?}"
    );
    assert_eq!(
        text_style.bg, bare_style.bg,
        "the text must carry no background; style: {text_style:?}"
    );
    assert!(
        !text_style.add_modifier.contains(Modifier::BOLD),
        "the text must not be bold; style: {text_style:?}"
    );
    assert_eq!(
        leading_style.bg, bare_style.bg,
        "no background band may start before the text; style: {leading_style:?}"
    );
    assert_eq!(
        trailing_style.bg, bare_style.bg,
        "no background band may fill the rest of the row; style: {trailing_style:?}"
    );
    assert_eq!(
        gap_style.bg, bare_style.bg,
        "the gap row must carry no background; style: {gap_style:?}"
    );
    Ok(())
}

/// A footer with no rows renders nothing: the rect keeps whatever the caller
/// painted there.
#[test]
fn test_runtime_picker_footer_render_is_a_noop_without_rows() -> Result<()> {
    // -- Setup & Fixtures
    const WIDTH: u16 = 58;
    let theme = TuiTheme::default();
    let footer = PickerFooter::measure(None, 4, WIDTH, 3);
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, 2))?;

    // -- Exec
    terminal.draw(|frame| {
        footer.render(frame, Rect::new(0, 0, WIDTH, 1), &theme);
    })?;
    let buffer = terminal.backend().buffer();
    // Row 1 sits outside the footer rect, so it stays as the backend left it.
    let bare_style = buffer[(0, 1)].style();

    // -- Check
    assert_eq!(footer.height(), 0, "no text must claim no rows");
    assert_eq!(
        buffer[(0, 0)].symbol(),
        " ",
        "an empty footer must paint no text"
    );
    assert_eq!(
        buffer[(0, 0)].style(),
        bare_style,
        "an empty footer must paint nothing"
    );
    Ok(())
}
