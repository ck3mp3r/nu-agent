use super::*;

// ---------------------------------------------------------------------------
// Index shifting — streaming_message_start and compaction_streaming_start
// ---------------------------------------------------------------------------

#[test]
fn shift_indices_after_eviction_shifts_or_drops_streaming_starts() -> Result<()> {
    // -- Setup & Fixtures
    // (label, initial, evicted, expected)
    let cases: [(&str, Option<usize>, usize, Option<usize>); 5] = [
        ("assistant shifted", Some(2010), 2000, Some(10)),
        ("assistant evicted", Some(5), 2000, None),
        ("assistant none", None, 2000, None),
        ("summary shifted", Some(2010), 2000, Some(10)),
        ("summary evicted", Some(5), 2000, None),
    ];

    // -- Exec & Check
    for (label, initial, evicted, expected) in cases {
        let mut state = AppState::default();
        if label.starts_with("assistant") {
            state.transcript.assistant_stream_start = initial;
        } else {
            state.transcript.summary_stream_start = initial;
        }

        state.transcript.shift_indices_after_eviction(evicted);

        let actual = if label.starts_with("assistant") {
            state.transcript.assistant_stream_start
        } else {
            state.transcript.summary_stream_start
        };
        assert_eq!(actual, expected, "{label}");
    }
    Ok(())
}
