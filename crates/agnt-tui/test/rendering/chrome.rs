use ratatui::{
    Terminal,
    backend::TestBackend,
    layout::{Margin, Rect},
    style::{Color, Style},
    text::Line,
    widgets::Block,
};

use crate::rendering::chrome::{Chrome, ContainerBorder};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

const WIDTH: u16 = 20;
const HEIGHT: u16 = 6;

fn area() -> Rect {
    Rect::new(0, 0, WIDTH, HEIGHT)
}

fn cell_row(terminal: &Terminal<TestBackend>, y: u16) -> String {
    let buffer = terminal.backend().buffer();
    (0..WIDTH)
        .map(|x| buffer[(x, y)].symbol())
        .collect::<String>()
}

/// THE Chrome::draw function SHALL return the bordered inner rect, one row and
/// one column inside the rect it was given.
#[test]
fn test_rendering_chrome_draw_returns_the_inner_rect() -> Result<()> {
    // -- Setup & Fixtures
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;
    let mut inner = Rect::default();

    // -- Exec
    terminal.draw(|frame| {
        inner = Chrome::default().draw(frame, area());
    })?;

    // -- Check
    assert_eq!(
        inner,
        area().inner(Margin {
            vertical: 1,
            horizontal: 1,
        }),
        "the content rect must sit one row and one column inside the container"
    );
    Ok(())
}

/// WHEN the container border is rounded, THE Chrome::draw function SHALL paint
/// rounded corner glyphs; WHEN it is plain, THE function SHALL paint the
/// terminal's default corner glyphs.
#[test]
fn test_rendering_chrome_draw_paints_the_requested_border_set() -> Result<()> {
    // -- Setup & Fixtures
    let mut rounded = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;
    let mut plain = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;

    // -- Exec
    rounded.draw(|frame| {
        Chrome::default()
            .with_border(ContainerBorder::Rounded)
            .draw(frame, area());
    })?;
    plain.draw(|frame| {
        Chrome::default()
            .with_border(ContainerBorder::Plain)
            .draw(frame, area());
    })?;

    // -- Check
    let rounded_top = cell_row(&rounded, 0);
    let rounded_bottom = cell_row(&rounded, HEIGHT - 1);
    assert!(
        rounded_top.starts_with('╭') && rounded_top.ends_with('╮'),
        "a rounded border must open with ╭ and close with ╮; rendered {rounded_top:?}"
    );
    assert!(
        rounded_bottom.starts_with('╰') && rounded_bottom.ends_with('╯'),
        "a rounded border must close with ╰ and ╯; rendered {rounded_bottom:?}"
    );
    let plain_top = cell_row(&plain, 0);
    assert!(
        plain_top.starts_with('┌') && plain_top.ends_with('┐'),
        "a plain border must keep the default corner glyphs; rendered {plain_top:?}"
    );
    Ok(())
}

/// WHEN a background style is given, THE Chrome::draw function SHALL paint the
/// rect background before the border, so every cell carries it; WHEN it is
/// absent, THE function SHALL leave the background untouched.
#[test]
fn test_rendering_chrome_draw_paints_the_background_before_the_border() -> Result<()> {
    // -- Setup & Fixtures
    let mut filled = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;
    let mut bare = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;

    // -- Exec
    filled.draw(|frame| {
        Chrome::default()
            .with_bg(Style::default().bg(Color::Magenta))
            .with_border(ContainerBorder::Rounded)
            .draw(frame, area());
    })?;
    bare.draw(|frame| {
        Chrome::default()
            .with_border(ContainerBorder::Rounded)
            .draw(frame, area());
    })?;

    // -- Check
    let buffer = filled.backend().buffer();
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            assert_eq!(
                buffer[(x, y)].style().bg,
                Some(Color::Magenta),
                "the background must reach cell ({x}, {y}) — border cells included"
            );
        }
    }
    let bare_buffer = bare.backend().buffer();
    assert_ne!(
        bare_buffer[(1, 1)].style().bg,
        Some(Color::Magenta),
        "without a background the interior must stay unpainted"
    );
    Ok(())
}

/// WHEN a border style is given, THE Chrome::draw function SHALL paint it on the
/// border cells only, leaving the interior alone.
#[test]
fn test_rendering_chrome_draw_paints_the_border_style_on_the_border() -> Result<()> {
    // -- Setup & Fixtures
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;

    // -- Exec
    terminal.draw(|frame| {
        Chrome::default()
            .with_border_style(Style::default().fg(Color::Cyan))
            .draw(frame, area());
    })?;

    // -- Check
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(0, 0)].style().fg,
        Some(Color::Cyan),
        "the border corner must carry the border style"
    );
    assert_eq!(
        buffer[(0, 1)].style().fg,
        Some(Color::Cyan),
        "the border rule must carry the border style"
    );
    assert_ne!(
        buffer[(1, 1)].style().fg,
        Some(Color::Cyan),
        "the interior must not carry the border style"
    );
    Ok(())
}

/// WHEN a title is given, THE Chrome::draw function SHALL paint it on the top
/// border row; WHEN it is absent, THE top row SHALL carry the border alone.
#[test]
fn test_rendering_chrome_draw_paints_the_title_on_the_top_row() -> Result<()> {
    // -- Setup & Fixtures
    let mut titled = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;
    let mut untitled = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;

    // -- Exec
    titled.draw(|frame| {
        Chrome::default()
            .with_title(Line::from("Events"))
            .draw(frame, area());
    })?;
    untitled.draw(|frame| {
        Chrome::default().draw(frame, area());
    })?;

    // -- Check
    let titled_top = cell_row(&titled, 0);
    let untitled_top = cell_row(&untitled, 0);
    assert!(
        titled_top.contains("Events"),
        "the title must paint on the top border row; rendered {titled_top:?}"
    );
    assert!(
        !untitled_top.contains("Events"),
        "an untitled container must not paint a title; rendered {untitled_top:?}"
    );
    Ok(())
}

/// WHEN a title style is given, THE Chrome::draw function SHALL paint the title
/// glyphs with it, leaving the rest of the top border to the border style.
#[test]
fn test_rendering_chrome_draw_paints_the_title_style() -> Result<()> {
    // -- Setup & Fixtures
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;

    // -- Exec
    terminal.draw(|frame| {
        Chrome::default()
            .with_title(Line::from("Events"))
            .with_title_style(Style::default().fg(Color::Cyan))
            .draw(frame, area());
    })?;

    // -- Check
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(1, 0)].style().fg,
        Some(Color::Cyan),
        "the first title glyph must carry the title style"
    );
    assert_eq!(
        buffer[(6, 0)].style().fg,
        Some(Color::Cyan),
        "the last title glyph must carry the title style"
    );
    assert_ne!(
        buffer[(WIDTH - 2, 0)].style().fg,
        Some(Color::Cyan),
        "the border beyond the title must keep the border style"
    );
    Ok(())
}

/// WHEN no title style is given, THE Chrome::draw function SHALL leave the title
/// glyphs on the default foreground, matching the unstyled border.
#[test]
fn test_rendering_chrome_draw_title_without_style_keeps_default_foreground() -> Result<()> {
    // -- Setup & Fixtures
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;

    // -- Exec
    terminal.draw(|frame| {
        Chrome::default()
            .with_title(Line::from("Events"))
            .draw(frame, area());
    })?;

    // -- Check
    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer[(1, 0)].style().fg,
        buffer[(0, 0)].style().fg,
        "without a title style the title must keep the border's default foreground"
    );
    Ok(())
}

/// WHEN clear is requested, THE Chrome::draw function SHALL wipe whatever the
/// area held earlier in the frame before it paints; WHEN it is absent, THE area
/// SHALL keep what it held.
#[test]
fn test_rendering_chrome_draw_clears_only_when_asked() -> Result<()> {
    // -- Setup & Fixtures
    let mut cleared = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;
    let mut kept = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;

    // -- Exec: paint noise, then draw the container over it in the same frame.
    cleared.draw(|frame| {
        frame.render_widget(
            Block::default().style(Style::default().bg(Color::Magenta)),
            area(),
        );
        Chrome::default().with_clear(true).draw(frame, area());
    })?;
    kept.draw(|frame| {
        frame.render_widget(
            Block::default().style(Style::default().bg(Color::Magenta)),
            area(),
        );
        Chrome::default().draw(frame, area());
    })?;

    // -- Check
    assert_ne!(
        cleared.backend().buffer()[(1, 1)].style().bg,
        Some(Color::Magenta),
        "a cleared container must wipe the area before painting"
    );
    assert_eq!(
        kept.backend().buffer()[(1, 1)].style().bg,
        Some(Color::Magenta),
        "a container that does not clear must keep the area's contents"
    );
    Ok(())
}

/// THE Chrome::draw function SHALL keep the interior free of border glyphs, so
/// callers can render their own content there.
#[test]
fn test_rendering_chrome_draw_leaves_the_interior_bare() -> Result<()> {
    // -- Setup & Fixtures
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;

    // -- Exec
    terminal.draw(|frame| {
        Chrome::default()
            .with_border(ContainerBorder::Rounded)
            .draw(frame, area());
    })?;

    // -- Check
    let buffer = terminal.backend().buffer();
    for y in 1..HEIGHT - 1 {
        assert_eq!(
            buffer[(0, y)].symbol(),
            "│",
            "row {y} must open with a rule"
        );
        assert_eq!(
            buffer[(WIDTH - 1, y)].symbol(),
            "│",
            "row {y} must close with a rule"
        );
        for x in 1..WIDTH - 1 {
            assert_eq!(
                buffer[(x, y)].symbol(),
                " ",
                "the interior must stay bare for the caller's content; cell ({x}, {y})"
            );
        }
    }
    Ok(())
}
