use ratatui::{Terminal, backend::TestBackend, layout::Rect, text::Line};

use crate::{rendering::theme::TuiTheme, runtime::panels::render_permission_controls};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

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

pub(crate) fn help_panel_visible_window(
    lines: &[Line<'_>],
    content_width: usize,
    scroll: usize,
    rows: usize,
) -> Vec<Line<'static>> {
    let mut visual_rows = Vec::new();
    let width = content_width.max(1);

    for line in lines {
        let text = line.to_string();
        if text.is_empty() {
            visual_rows.push(String::new());
            continue;
        }
        let chars = text.chars().collect::<Vec<_>>();
        for chunk in chars.chunks(width) {
            visual_rows.push(chunk.iter().collect::<String>());
        }
    }

    visual_rows
        .into_iter()
        .skip(scroll)
        .take(rows)
        .map(Line::from)
        .collect()
}
