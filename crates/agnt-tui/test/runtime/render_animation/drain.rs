use super::*;

use agnt_core::transcript::entry::TranscriptEntry;

/// Build a chunk with the producer's two facts kept apart: `delta` is the new
/// suffix, `aggregated` the accumulated text.
fn chunk(delta: &str, aggregated: &str) -> SystemEvent {
    SystemEvent::InferenceChunk {
        delta: delta.to_string(),
        aggregated: aggregated.to_string(),
    }
}

/// The assistant entries' text, in transcript order.
fn assistant_texts(coordinator: &RuntimeCoordinator) -> Vec<String> {
    coordinator
        .state()
        .transcript
        .entries()
        .iter()
        .filter(|entry| {
            matches!(
                entry,
                TranscriptEntry::Message(message) if message.role == MessageRole::Assistant
            )
        })
        .map(entry_text)
        .collect()
}

/// `drain_transport` coalesces consecutive assistant chunks onto the latest
/// aggregated text, preserves order across mixed events, and passes a single
/// chunk through unchanged.
#[test]
fn drain_transport_coalescing_and_order() -> Result<()> {
    // -- Setup & Fixtures: a realistic incremental stream in one drain cycle.
    let mut coalescing = RuntimeCoordinator::new(120, 30, Some(true));
    for (delta, aggregated) in [
        ("a", "a"),
        ("b", "ab"),
        ("c", "abc"),
        ("d", "abcd"),
        ("e", "abcde"),
    ] {
        coalescing.enqueue_event(chunk(delta, aggregated));
    }

    // -- Exec
    coalescing.drain_transport();

    // -- Check: only the latest aggregated text lands in the transcript.
    let texts = assistant_texts(&coalescing);
    assert_eq!(texts, vec!["abcde".to_string()], "got {texts:?}");

    // -- Setup & Fixtures: the same stream drained chunk by chunk, so the
    // intermediate states are observable.
    let mut stepwise = RuntimeCoordinator::new(120, 30, Some(true));
    let stepwise_chunks = [("a", "a"), ("b", "ab")];

    // -- Exec & Check: every cycle shows the accumulated text of its chunk.
    for (index, (delta, aggregated)) in stepwise_chunks.iter().enumerate() {
        stepwise.enqueue_event(chunk(delta, aggregated));
        stepwise.drain_transport();
        assert_eq!(
            assistant_texts(&stepwise),
            vec![aggregated.to_string()],
            "drain cycle {index}"
        );
    }

    // -- Exec: two more chunks in ONE cycle coalesce onto the newest one.
    stepwise.enqueue_event(chunk("c", "abc"));
    stepwise.enqueue_event(chunk("d", "abcd"));
    stepwise.drain_transport();

    // -- Check: the coalesced cycle still shows one growing block — the newest
    // aggregated text, not the intermediate one.
    assert_eq!(assistant_texts(&stepwise), vec!["abcd".to_string()]);

    // -- Setup & Fixtures: mixed events separated by a Tick.
    let mut mixed = RuntimeCoordinator::new(120, 30, Some(true));
    mixed.enqueue_event(chunk("hello", "hello"));
    mixed.enqueue_event(SystemEvent::Tick);

    // -- Exec & Check: the first cycle renders the first prefix.
    mixed.drain_transport();
    assert_eq!(assistant_texts(&mixed), vec!["hello".to_string()]);

    // -- Exec: the second chunk of the same message arrives after the Tick.
    mixed.enqueue_event(chunk(" world", "hello world"));
    mixed.drain_transport();

    // -- Check: the block grew — same single block, accumulated text.
    assert_eq!(assistant_texts(&mixed), vec!["hello world".to_string()]);

    // -- Setup & Fixtures: a single assistant chunk.
    let mut single = RuntimeCoordinator::new(120, 30, Some(true));
    single.enqueue_event(chunk("solo", "solo"));

    // -- Exec
    single.drain_transport();

    // -- Check
    assert_eq!(assistant_texts(&single), vec!["solo".to_string()]);
    Ok(())
}
