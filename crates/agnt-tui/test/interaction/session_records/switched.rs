//! The `Switched` fold: the rows of the session that left go, the banner marks
//! the new one, and the replay that follows lands below it. Its siblings in the
//! session vocabulary — an agent switch, a model switch, the close — fold
//! nothing at all.

use super::session_records_support_test::*;

#[test]
fn test_interaction_session_records_switched_clears_and_banners() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    fold_record(
        &mut state,
        prompt_record("hi", Some(1_700_000_000_000), None, None),
    );
    assert_eq!(
        state.transcript.len(),
        1,
        "the session being left must have a row on screen before the fold"
    );

    // -- Exec
    let changed = fold_record(&mut state, session_switched());

    // -- Check
    assert!(changed, "a session switch is a fold the surface handles");
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
        "the rows of the session that left are gone and the banner marks the new one"
    );
    Ok(())
}

#[test]
fn test_interaction_session_records_switched_prompt_after_switched_follows_banner() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();
    fold_record(
        &mut state,
        prompt_record("old", Some(1_700_000_000_000), None, None),
    );
    fold_record(&mut state, session_switched());

    // -- Exec
    let changed = fold_record(
        &mut state,
        prompt_record("new", Some(1_700_000_000_500), None, None),
    );

    // -- Check
    assert!(changed, "the replayed prompt is a row the fold places");
    let entries = state.transcript.entries();
    assert_eq!(
        entries.len(),
        2,
        "the replayed prompt lands after the banner, not below the old session"
    );
    assert!(
        entries
            .first()
            .is_some_and(|entry| matches!(entry, TranscriptEntry::Banner(_))),
        "the banner keeps its place at the head of the new session"
    );
    let Some(entry) = entries.get(1) else {
        return Err("expected the replayed prompt row".into());
    };
    assert_eq!(
        entry_text(entry),
        "new",
        "the row that follows the banner is the replayed prompt"
    );
    Ok(())
}

#[test]
fn test_interaction_session_records_switched_siblings_make_no_change() -> Result<()> {
    // -- Setup & Fixtures: one prompt row the siblings must leave untouched.
    let mut state = AppState::default();
    fold_record(
        &mut state,
        prompt_record("hi", Some(1_700_000_000_000), None, None),
    );

    // -- Exec
    let agent_switched = fold_record(&mut state, session_agent_switched());
    let model_switched = fold_record(&mut state, session_model_switched());
    let ended = fold_record(&mut state, session_ended());

    // -- Check: the switch vocabulary's siblings are not transcript folds. Each
    // verdict is a no-op, so the transcript still holds the one prompt row.
    assert!(!agent_switched, "an agent switch folds nothing");
    assert!(!model_switched, "a model switch folds nothing");
    assert!(!ended, "the session close folds nothing");
    let entries = state.transcript.entries();
    assert_eq!(
        entries.len(),
        1,
        "the one prompt row is all the transcript holds"
    );
    assert!(
        entries
            .iter()
            .all(|entry| !matches!(entry, TranscriptEntry::Banner(_))),
        "no sibling record pushes a banner"
    );
    let Some(entry) = entries.first() else {
        return Err("expected the prompt row".into());
    };
    assert_eq!(
        entry_text(entry),
        "hi",
        "the surviving row is the prompt the fold placed before them"
    );
    Ok(())
}
