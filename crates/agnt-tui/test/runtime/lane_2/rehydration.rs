use super::*;

/// Hydration rehydrates the used tokens from the history metadata: a known max
/// shows the ratio immediately, and a missing usage falls back to zero.
#[test]
fn lane_2_rehydrates_used_tokens() -> Result<()> {
    // -- Setup & Fixtures: history metadata with a total.
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    coordinator.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("user", "hello"), {
            let mut s = UiMessageSnapshot::new("assistant", "history");
            s.usage = Some(UiMessageUsageSnapshot {
                input_tokens: None,
                output_tokens: None,
                total_tokens: Some(444),
            });
            s
        }],
        None,
    );

    // -- Check: the used tokens are rehydrated.
    let lane_2 = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text.chars().count(), 120);
    assert!(text.ends_with("444"));

    // -- Setup & Fixtures: a known max.
    let mut known = RuntimeCoordinator::new(120, 30, Some(true));
    known
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(1000));

    // -- Exec
    known.hydrate_transcript_from_messages(
        vec![{
            let mut s = UiMessageSnapshot::new("assistant", "history");
            s.usage = Some(UiMessageUsageSnapshot {
                input_tokens: None,
                output_tokens: None,
                total_tokens: Some(250),
            });
            s
        }],
        None,
    );

    // -- Check: the ratio is shown immediately.
    let lane_2 = crate::runtime::lane_2_status_line_for_test(known.state(), 120);
    let text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.ends_with("250 (25%)"));

    // -- Setup & Fixtures: no usage metadata and no max.
    let mut fallback = RuntimeCoordinator::new(120, 30, Some(true));

    // -- Exec
    fallback.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", "history")],
        None,
    );

    // -- Check: the fallback shows zero.
    let lane_2 = crate::runtime::lane_2_status_line_for_test(fallback.state(), 120);
    let text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(text.chars().count(), 120);
    assert!(text.ends_with("0"));

    // -- Setup & Fixtures: no usage metadata but a known max.
    let mut known_max = RuntimeCoordinator::new(120, 30, Some(true));
    known_max
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(100));

    // -- Exec
    known_max.hydrate_transcript_from_messages(
        vec![UiMessageSnapshot::new("assistant", "history")],
        None,
    );

    // -- Check: the ratio is shown, not the fallback.
    let lane_2 = crate::runtime::lane_2_status_line_for_test(known_max.state(), 120);
    let text: String = lane_2.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(text.ends_with("0 (0%)"));
    Ok(())
}

/// A live turn's usage replaces the rehydrated value.
#[test]
fn lane_2_rehydrate_replaced_by_live_turn_usage() -> Result<()> {
    // -- Setup & Fixtures
    let mut coordinator = RuntimeCoordinator::new(120, 30, Some(true));
    coordinator
        .state
        .status
        .tokens
        .set_context_window_max_tokens(Some(100));
    coordinator.hydrate_transcript_from_messages(
        vec![{
            let mut s = UiMessageSnapshot::new("assistant", "history");
            s.usage = Some(UiMessageUsageSnapshot {
                input_tokens: None,
                output_tokens: None,
                total_tokens: Some(7),
            });
            s
        }],
        None,
    );

    // -- Exec & Check: the rehydrated value.
    let hydrated = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let hydrated_text: String = hydrated.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(hydrated_text.ends_with("7 (7%)"));

    // -- Exec: a live turn.
    coordinator.enqueue_ui_event(UiEvent::LlmCompleted {
        response_chars: 20,
        tool_calls: 0,
        input_tokens: 8,
        output_tokens: 32,
        total_tokens: 40,
    });
    coordinator.drain_transport();

    // -- Check: the live value replaces the rehydrated one.
    let live = crate::runtime::lane_2_status_line_for_test(coordinator.state(), 120);
    let live_text: String = live.spans.iter().map(|s| s.content.as_ref()).collect();
    assert!(live_text.ends_with("40 (40%)"));
    Ok(())
}
