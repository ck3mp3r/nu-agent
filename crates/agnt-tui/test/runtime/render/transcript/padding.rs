use super::*;

use agnt_core::protocol::contracts::UiMessageSnapshot;

/// A rehydrated transcript whose first entry is a user message bottom-aligns
/// with padding rows. Those padding rows must stay on the base background —
/// they must not inherit the user turn's surface.
#[test]
fn test_padding_rows_do_not_inherit_entry_zero_surface() -> Result<()> {
    // -- Setup & Fixtures
    let mut coord = RuntimeCoordinator::new(40, 20, None);
    let columns = 40u16;
    let rows = 20u16;
    let viewport = rows as usize;
    // Hydration without a leading logo leaves the user message as entry 0, the
    // entry whose turn paints the user surface.
    coord.hydrate_transcript_from_messages(vec![UiMessageSnapshot::new("user", "hi")], None);
    assert_eq!(coord.state.transcript.len(), 1, "one hydrated entry");
    assert!(
        matches!(
            coord.state.transcript.entries().first(),
            Some(TranscriptEntry::Message(message)) if message.role == MessageRole::User
        ),
        "entry 0 must be a user message"
    );
    // One content row plus the user turn's trailing separator row.
    let content_rows = 2usize;
    let padding = viewport - content_rows;
    let top_padding = padding / 2;
    let user_row = top_padding;
    let bottom_padding_start = top_padding + content_rows;

    // -- Exec
    let terminal = render_transcript_pane_to_backend(&mut coord, columns, rows, true, 0);

    // -- Check
    let user_bg = coord.theme.row_user_bg;
    let base = coord.theme.base;
    for y in 0..top_padding {
        assert_eq!(
            cell_bg(&terminal, 1, y as u16),
            Some(base),
            "top padding row {y} must stay on the base background"
        );
    }
    assert_eq!(
        cell_bg(&terminal, 1, user_row as u16),
        Some(user_bg),
        "the user message's own content row must keep the user background"
    );
    for y in bottom_padding_start..viewport {
        assert_eq!(
            cell_bg(&terminal, 1, y as u16),
            Some(base),
            "bottom padding row {y} must stay on the base background"
        );
    }
    Ok(())
}
