use super::*;

use crate::state::{InputMode, PaneFocus};

/// The `>` cursor marker maps through the paint window: when the pane
/// bottom-aligns a short transcript, the marker rides the centering pad with the
/// content — never the blank padding row where the pad-less mapping put it.
#[test]
fn short_transcript_cursor_marker_rides_the_painted_content_row() -> Result<()> {
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
    assert!(top_pad > 0, "fixture must produce centering padding");

    // The cursor sits on the content's first visual row. Detach from the tail so
    // the render sync does not pin the cursor to the last visual row.
    coord.state.scroll.following_tail = false;
    coord.state.scroll.pane_focus = PaneFocus::Transcript;
    coord.state.input.mode = InputMode::Normal;
    coord.state.scroll.cursor_visual_row = 0;

    // -- Exec: render the pane without tail-following, so the window starts at
    // absolute row 0 and the centering pad puts it at screen row `top_pad`.
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, rows, false, 0);

    // -- Check: the marker sits on the cursor row's painted position.
    assert_eq!(
        cell_symbol(&terminal, 0, top_pad as u16),
        ">",
        "the marker must ride the centering pad with the cursor's content row"
    );

    // -- Check: no padding row carries the marker.
    for y in 0..rows as usize {
        if y == top_pad {
            continue;
        }
        for x in 0..list_width {
            assert_ne!(
                cell_symbol(&terminal, x, y as u16),
                ">",
                "padding screen row {y} col {x} must not carry the marker"
            );
        }
    }
    Ok(())
}
