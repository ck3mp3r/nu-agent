use super::*;

/// `drain_transport` coalesces consecutive assistant messages, preserves order
/// across mixed events, and passes a single assistant message through
/// unchanged.
#[test]
fn drain_transport_coalescing_and_order() -> Result<()> {
    // -- Setup & Fixtures: consecutive assistant messages.
    let mut coalescing = RuntimeCoordinator::new(120, 30, Some(true));
    for text in ["a", "ab", "abc", "abcd", "abcde"] {
        coalescing.enqueue_ui_event(UiEvent::AssistantMessage {
            delta: text.to_string(),
            text: text.to_string(),
        });
    }

    // -- Exec
    coalescing.drain_transport();

    // -- Check: only the final coalesced text lands in the transcript.
    let texts: Vec<String> = coalescing
        .state()
        .transcript
        .blocks()
        .iter()
        .map(|block| block.source.plain_text())
        .collect();
    assert!(texts.contains(&"abcde".to_string()), "got {texts:?}");
    assert!(!texts.contains(&"abcd".to_string()), "got {texts:?}");

    // -- Setup & Fixtures: mixed events separated by a Tick.
    let mut mixed = RuntimeCoordinator::new(120, 30, Some(true));
    mixed.enqueue_ui_event(UiEvent::AssistantMessage {
        delta: "hello".to_string(),
        text: "hello".to_string(),
    });
    mixed.enqueue_ui_event(UiEvent::Tick);
    mixed.enqueue_ui_event(UiEvent::AssistantMessage {
        delta: "world".to_string(),
        text: "world".to_string(),
    });

    // -- Exec
    mixed.drain_transport();

    // -- Check: the second message replaces the first.
    let texts: Vec<String> = mixed
        .state()
        .transcript
        .blocks()
        .iter()
        .filter(|b| {
            matches!(
                b.source,
                BlockSource::Markdown {
                    role: MessageRole::Assistant,
                    ..
                }
            )
        })
        .map(|block| block.source.plain_text())
        .collect();
    assert_eq!(texts, vec!["world"]);

    // -- Setup & Fixtures: a single assistant message.
    let mut single = RuntimeCoordinator::new(120, 30, Some(true));
    single.enqueue_ui_event(UiEvent::AssistantMessage {
        delta: "solo".to_string(),
        text: "solo".to_string(),
    });

    // -- Exec
    single.drain_transport();

    // -- Check
    let texts: Vec<String> = single
        .state()
        .transcript
        .blocks()
        .iter()
        .filter(|b| {
            matches!(
                b.source,
                BlockSource::Markdown {
                    role: MessageRole::Assistant,
                    ..
                }
            )
        })
        .map(|block| block.source.plain_text())
        .collect();
    assert_eq!(texts, vec!["solo"]);
    Ok(())
}
