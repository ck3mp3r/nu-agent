use crate::agent::ui::tui::rendering::selection::TranscriptSelection;

#[test]
fn starts_at_provided_transcript_index_for_gg_and_g_positions() {
    let at_top = TranscriptSelection::new(0);
    assert_eq!(at_top.anchor(), 0);
    assert_eq!(at_top.cursor(), 0);
    assert_eq!(at_top.normalized_range(), (0, 0));

    let at_bottom = TranscriptSelection::new(42);
    assert_eq!(at_bottom.anchor(), 42);
    assert_eq!(at_bottom.cursor(), 42);
    assert_eq!(at_bottom.normalized_range(), (42, 42));
}

#[test]
fn range_normalizes_when_anchor_is_greater_than_cursor() {
    let mut selection = TranscriptSelection::new(7);
    selection.set_cursor(2);

    assert_eq!(selection.anchor(), 7);
    assert_eq!(selection.cursor(), 2);
    assert_eq!(selection.normalized_range(), (2, 7));
}

#[test]
fn yank_payload_joins_selected_lines_with_newlines() {
    let mut selection = TranscriptSelection::new(1);
    selection.set_cursor(3);

    let transcript = vec!["line 0", "line 1", "line 2", "line 3", "line 4"];
    let payload = selection.yank_payload(&transcript);

    assert_eq!(payload, "line 1\nline 2\nline 3");
}

#[test]
fn yank_payload_is_deterministic_for_empty_and_out_of_range_transcript() {
    let selection = TranscriptSelection::new(0);
    let empty: Vec<&str> = Vec::new();
    assert_eq!(selection.yank_payload(&empty), "");

    let out_of_range = TranscriptSelection::new(9);
    let transcript = vec!["line 0", "line 1"];
    assert_eq!(out_of_range.yank_payload(&transcript), "");
}

#[test]
fn bounded_range_clamps_cursor_to_last_transcript_line() {
    let mut selection = TranscriptSelection::new(1);
    selection.set_cursor(99);

    assert_eq!(selection.bounded_range(4), Some((1, 3)));
}
