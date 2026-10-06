use super::selection::TranscriptSelection;

#[test]
fn starts_at_provided_transcript_index() {
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
