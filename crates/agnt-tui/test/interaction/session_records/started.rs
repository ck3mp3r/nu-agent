//! The `Started` fold: a fresh mint clears to the banner, a resume keeps the
//! history it loaded.

use super::session_records_support_test::*;

#[test]
fn test_interaction_session_records_started_unhydrated_clears_and_banners() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    fold_record(
        &mut state,
        prompt_record("hi", Some(1_700_000_000_000), None, None),
    );
    fold_record(
        &mut state,
        SessionRecord::Inference(InferenceRecord::Completed {
            usage: Usage {
                input_tokens: 12,
                output_tokens: 34,
                total_tokens: Some(46),
                cost: None,
                cost_complete: false,
            },
        }),
    );
    state.scroll.scroll_offset = 7;
    state.scroll.following_tail = false;
    assert_eq!(
        state.transcript.len(),
        1,
        "the previous session's row must be on screen before the fold"
    );

    // -- Exec
    let changed = fold_record(&mut state, session_started(false));

    // -- Check
    assert!(
        changed,
        "a fresh session start is a fold the surface handles"
    );
    let entries = state.transcript.entries();
    assert_eq!(
        entries.len(),
        1,
        "the transcript holds exactly the startup banner"
    );
    let Some(entry) = entries.first() else {
        return Err("expected the startup banner".into());
    };
    assert!(
        matches!(entry, TranscriptEntry::Banner(_)),
        "the one remaining entry must be the banner, not a session row"
    );
    assert_eq!(state.scroll.scroll_offset, 0, "the scroll offset resets");
    assert!(
        state.scroll.following_tail,
        "the tail follows the new session"
    );
    assert_eq!(state.status.tokens.latest_input_tokens, None);
    assert_eq!(state.status.tokens.latest_output_tokens, None);
    assert_eq!(state.status.tokens.latest_total_tokens, None);
    Ok(())
}

#[test]
fn test_interaction_session_records_started_hydrated_keeps_transcript() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    fold_record(
        &mut state,
        prompt_record("hi", Some(1_700_000_000_000), None, None),
    );
    state.status.tokens.record_token_usage(12, 34, Some(46));

    // -- Exec
    let changed = fold_record(&mut state, session_started(true));

    // -- Check
    assert!(!changed, "a hydrated start changes nothing on screen");
    let entries = state.transcript.entries();
    assert_eq!(entries.len(), 1, "the loaded history stays as it arrived");
    assert!(
        entries
            .iter()
            .all(|entry| !matches!(entry, TranscriptEntry::Banner(_))),
        "a hydrated start pushes no banner of its own"
    );
    assert_eq!(
        state.status.tokens.latest_total_tokens,
        Some(46),
        "the token display keeps the resumed totals"
    );
    Ok(())
}
