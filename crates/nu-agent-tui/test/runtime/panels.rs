use ratatui::text::Line;

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
