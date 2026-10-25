use super::*;
use crate::state::selection::TranscriptSelection;

/// The yank payload for a selection equals the rows the renderer displays for
/// the same range — lane prefixes, separator blanks, and wrapped rows included.
#[test]
fn yank_payload_matches_displayed_rows() -> Result<()> {
    // -- Setup & Fixtures: a user prompt that wraps plus an assistant reply.
    let mut coord = RuntimeCoordinator::new(40, 20, None);
    coord.state.transcript.push_block(Block {
        source: BlockSource::Markdown {
            role: MessageRole::User,
            markdown: "a user prompt long enough to wrap at the narrow pane width".to_string(),
        },
        lane: L::Marker("▏"),
        fill: Fill::Full,
        status: None,
    });
    coord.state.transcript.push_block(Block {
        source: BlockSource::Markdown {
            role: MessageRole::Assistant,
            markdown: "assistant reply".to_string(),
        },
        lane: L::Blank,
        fill: Fill::None,
        status: None,
    });
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
