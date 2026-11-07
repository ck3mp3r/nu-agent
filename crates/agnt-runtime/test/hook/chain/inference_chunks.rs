//! The inference-chunk payload: the two facts producer A publishes.

use super::*;

/// Producer A publishes both facts per streamed delta: `delta` is the newly
/// received text, `aggregated` the accumulated stream text. The fixture drives
/// a real streamed turn through the real hook chain, so a producer that drops
/// or swaps either field fails here.
#[tokio::test]
async fn on_text_delta_publishes_new_text_and_accumulated_text() -> Result<()> {
    // -- Setup & Fixtures
    let fx = RepetitionTurnFixture::new()?;
    // Subscribe BEFORE driving the stream — broadcast sends are not buffered
    // for later subscribers.
    let mut rx = fx.bus.ui_event().subscribe();
    fx.queue_turn(vec![vec![
        MockStreamEvent::Text("hello".to_string()),
        MockStreamEvent::Text(" world".to_string()),
        MockStreamEvent::Text("!".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    // -- Exec
    let items = fx.turn(false).await?;

    // -- Check: the turn ran to completion.
    assert!(
        !items.iter().any(|item| item.is_err()),
        "the streamed turn must complete, got {items:?}"
    );

    // -- Check: one chunk per delta, each carrying its own pair.
    assert_eq!(
        inference_chunks(&mut rx),
        vec![
            ("hello".to_string(), "hello".to_string()),
            (" world".to_string(), "hello world".to_string()),
            ("!".to_string(), "hello world!".to_string()),
        ],
        "delta is the new text and aggregated the accumulated text"
    );
    Ok(())
}
