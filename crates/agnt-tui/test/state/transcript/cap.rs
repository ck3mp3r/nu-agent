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
    assert!(empty.transcript.blocks().is_empty());

    // Below the cap: nothing is evicted and every pushed block survives.
    let mut below = AppState::default();
    let push_count = MAX_TRANSCRIPT_BLOCKS / 2;
    for i in 0..push_count {
        push_user(&mut below, &format!("entry {i}"));
    }
    assert_eq!(below.transcript.len(), push_count);
    assert_eq!(content_block_count(&below), push_count);
    let content_blocks = below.transcript.blocks();
    assert_eq!(content_blocks[0].source.plain_text(), "entry 0");
    assert_eq!(
        content_blocks[push_count - 1].source.plain_text(),
        format!("entry {}", push_count - 1)
    );

    // Over the cap: the oldest block is evicted and the total stays at the cap.
    let mut over = AppState::default();
    for i in 0..=MAX_TRANSCRIPT_BLOCKS {
        push_user(&mut over, &format!("entry {i}"));
    }
    assert_eq!(over.transcript.len(), MAX_TRANSCRIPT_BLOCKS);
    let first = over
        .transcript
        .blocks()
        .first()
        .ok_or("should have block")?;
    assert_ne!(first.source.plain_text(), "entry 0");

    // Alternating turn roles: the cap holds regardless of role mix.
    let mut alternating = AppState::default();
    for i in 0..MAX_TRANSCRIPT_BLOCKS / 2 {
        push_user(&mut alternating, &format!("user {i}"));
        push_assistant(&mut alternating, &format!("assistant {i}"));
    }
    push_user(&mut alternating, "overflow");
    assert_eq!(alternating.transcript.len(), MAX_TRANSCRIPT_BLOCKS);
    Ok(())
}
