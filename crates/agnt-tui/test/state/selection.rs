use super::selection::TranscriptSelection;

#[test]
fn transcript_selection_tracks_anchor_cursor_and_normalized_range() {
    // -- Setup & Fixtures: selections anchored at the top and at the bottom.
    let cases = [(0usize, (0usize, 0usize)), (42, (42, 42))];

    // -- Exec & Check
    for (index, expected_range) in cases {
        let selection = TranscriptSelection::new(index);
        assert_eq!(selection.anchor(), index);
        assert_eq!(selection.cursor(), index);
        assert_eq!(selection.normalized_range(), expected_range);
    }

    // -- Setup & Fixtures: a cursor moved above the anchor.
    let mut selection = TranscriptSelection::new(7);

    // -- Exec
    selection.set_cursor(2);

    // -- Check
    assert_eq!(selection.anchor(), 7);
    assert_eq!(selection.cursor(), 2);
    assert_eq!(selection.normalized_range(), (2, 7));
}
