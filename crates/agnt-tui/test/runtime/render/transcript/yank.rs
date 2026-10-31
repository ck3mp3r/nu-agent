use super::*;
use crate::state::InputMode;
use crate::state::selection::TranscriptSelection;

/// The yank payload for a selection equals the rows the renderer displays for
/// the same range — lane prefixes, separator blanks, and wrapped rows included.
#[test]
fn yank_payload_matches_displayed_rows() -> Result<()> {
    // -- Setup & Fixtures: a user prompt that wraps plus an assistant reply.
    let mut coord = RuntimeCoordinator::new(40, 20, None);
    coord.state.transcript.push_entry(user_entry(
        "a user prompt long enough to wrap at the narrow pane width",
    ));
    coord
        .state
        .transcript
        .push_entry(assistant_entry("assistant reply"));
    let columns = 40u16;
    let list_width = columns - 2;
    coord
        .state
        .transcript
        .rebuild_height_index(list_width as usize);
    let total = coord.state.transcript.total_visual_rows();
    assert!(total > 1, "fixture must span several visual rows");

    // -- Exec: render the pane with a viewport exactly as tall as the content,
    // so no bottom-align padding rows are added.
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, total as u16, true, 0);
    let displayed: Vec<String> = (0..total as u16)
        .map(|y| {
            let mut row = String::new();
            for x in 0..list_width {
                if let Some(cell) = terminal.backend().buffer().cell((x, y)) {
                    row.push(cell.symbol().chars().next().unwrap_or(' '));
                }
            }
            row.trim_end().to_string()
        })
        .collect();

    // -- Exec: yank the whole transcript.
    coord.state.scroll.transcript_area = Rect::new(0, 0, columns, total as u16);
    let mut sel = TranscriptSelection::new(0);
    sel.set_cursor(total - 1);
    coord.state.scroll.selection = Some(sel);
    let payload = coord
        .state
        .yank_selection_payload()
        .ok_or("should have a yank payload")?;

    // -- Check
    assert_eq!(payload, displayed.join("\n"));
    Ok(())
}

/// The highlight and the yank identify the same rows when the pane bottom-aligns
/// a short transcript: the content's first row paints at the centering pad, so
/// the highlight must ride the pad too — never the blank rows above the content.
#[test]
fn short_transcript_highlight_and_yank_share_the_painted_rows() -> Result<()> {
    // -- Setup & Fixtures: a transcript shorter than the viewport, so the pane
    // centers it with padding rows above and below.
    let mut coord = RuntimeCoordinator::new(40, 24, None);
    coord.state.transcript.push_entry(user_entry(
        "a user prompt long enough to wrap at the narrow pane width",
    ));
    coord
        .state
        .transcript
        .push_entry(assistant_entry("assistant reply"));
    let columns = 40u16;
    let rows = 24u16;
    let list_width = columns - 2;
    coord
        .state
        .transcript
        .rebuild_height_index(list_width as usize);
    let total = coord.state.transcript.total_visual_rows();
    assert!(
        total < rows as usize,
        "fixture must be shorter than the viewport so the pane pads it"
    );
    let top_pad = (rows as usize - total) / 2;

    // The selection covers exactly the content's visual rows.
    let mut sel = TranscriptSelection::new(0);
    sel.set_cursor(total - 1);
    coord.state.scroll.selection = Some(sel);
    coord.state.input.mode = InputMode::Visual;
    coord.state.scroll.transcript_area = Rect::new(0, 0, columns, rows);

    // -- Exec: render the pane without tail-following, so the window starts at
    // absolute row 0 and the centering pad puts it at screen row `top_pad`.
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, rows, false, 0);
    let displayed: Vec<String> = (0..total)
        .map(|row| {
            let y = (top_pad + row) as u16;
            let mut text = String::new();
            for x in 0..list_width {
                if let Some(cell) = terminal.backend().buffer().cell((x, y)) {
                    text.push(cell.symbol().chars().next().unwrap_or(' '));
                }
            }
            text.trim_end().to_string()
        })
        .collect();

    // -- Exec: yank the same selection.
    let payload = coord
        .state
        .yank_selection_payload()
        .ok_or("should have a yank payload")?;

    // -- Check: the copied bytes are the painted rows of the selection.
    assert_eq!(payload, displayed.join("\n"));

    // -- Check: the highlight covers the content rows' painted positions and
    // never the centering padding.
    let highlight = coord.theme.selection_bg.bg;
    for y in 0..rows as usize {
        let selected_row = (top_pad..top_pad + total).contains(&y);
        for x in 0..list_width {
            let bg = cell_bg(&terminal, x, y as u16);
            if selected_row {
                assert_eq!(bg, highlight, "content screen row {y} col {x} is selected");
            } else {
                assert_ne!(
                    bg, highlight,
                    "padding screen row {y} col {x} is not selected"
                );
            }
        }
    }
    Ok(())
}
