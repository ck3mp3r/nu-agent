use super::*;

// ---------------------------------------------------------------------------
// Cap enforcement
// ---------------------------------------------------------------------------

#[test]
fn transcript_cap_enforces_bound_and_evicts_oldest() -> Result<()> {
    // -- Setup & Fixtures
    // Empty transcript: enforce is a no-op and must not panic.
    let mut empty = AppState::default();

    // -- Exec & Check
    empty.transcript.enforce_transcript_cap();
    assert!(empty.transcript.entries().is_empty());

    // Below the cap: nothing is evicted and every pushed entry survives.
    let mut below = AppState::default();
    let push_count = MAX_TRANSCRIPT_ENTRIES / 2;
    for i in 0..push_count {
        push_user(&mut below, &format!("entry {i}"));
    }
    assert_eq!(below.transcript.len(), push_count);
    assert_eq!(content_entry_count(&below), push_count);
    let entries = below.transcript.entries();
    let first = entries.first().ok_or("should have entry")?;
    let last = entries.last().ok_or("should have entry")?;
    assert_eq!(message_text(first), "entry 0");
    assert_eq!(message_text(last), format!("entry {}", push_count - 1));

    // Over the cap: the oldest entry is evicted and the total stays at the cap.
    let mut over = AppState::default();
    for i in 0..=MAX_TRANSCRIPT_ENTRIES {
        push_user(&mut over, &format!("entry {i}"));
    }
    assert_eq!(over.transcript.len(), MAX_TRANSCRIPT_ENTRIES);
    let first = over
        .transcript
        .entries()
        .first()
        .ok_or("should have entry")?;
    assert_ne!(message_text(first), "entry 0");

    // Alternating turn roles: the cap holds regardless of role mix.
    let mut alternating = AppState::default();
    for i in 0..MAX_TRANSCRIPT_ENTRIES / 2 {
        push_user(&mut alternating, &format!("user {i}"));
        push_assistant(&mut alternating, &format!("assistant {i}"));
    }
    push_user(&mut alternating, "overflow");
    assert_eq!(alternating.transcript.len(), MAX_TRANSCRIPT_ENTRIES);
    Ok(())
}

/// The markdown text of a message entry, or the empty string for any other
/// entry family.
fn message_text(entry: &TranscriptEntry) -> String {
    match entry {
        TranscriptEntry::Message(message) => message.markdown.clone(),
        TranscriptEntry::Tool(_) | TranscriptEntry::Notice(_) | TranscriptEntry::Banner(_) => {
            String::new()
        }
    }
}
