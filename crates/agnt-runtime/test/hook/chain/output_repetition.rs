use super::*;

/// Intra-stream detection stops the stream on the FIRST detection: a
/// repetitive stream is killed mid-turn with a Cancelled carrying the
/// `OUTPUT_REPETITION_STOP_PREFIX` reason, and NO hook-side warning is
/// published (the executor surfaces the stop reason).
#[tokio::test]
async fn on_text_delta_repetition_stops_mid_stream_on_first_detection() -> Result<()> {
    // -- Setup & Fixtures
    // 12 deltas x 21 bytes = 252 bytes: the growth gate opens at delta 5
    // (105 bytes) where the suffix check detects the 5-aligned 21-byte-period
    // repetition → `check_aggregated` returns Stop → the stream dies
    // mid-turn. No retry turn is queued (the executor's stop-to-steering
    // retry is exercised in executor tests, not here).
    let fx = RepetitionTurnFixture::new()?;
    fx.queue_turn(vec![identical_text_turn(12)]);
    // Subscribe BEFORE driving the stream — broadcast sends are not buffered
    // for later subscribers.
    let mut warning_rx = fx.bus.ui_event().subscribe();

    // -- Exec: drive the stream; the repetitive turn must die mid-stream.
    let items = fx.turn(false).await?;

    // -- Check: the stream errors with a Cancelled carrying the prefix.
    let mut error_stream = None;
    for item in items {
        if let Err(err) = item {
            error_stream = Some(err);
            break;
        }
    }
    let error = error_stream.ok_or("the first intra-stream detection must stop the run")?;
    let rig::completion::PromptError::Cancelled { reason, .. } = error else {
        return Err("the mid-stream stop must surface Cancelled".into());
    };
    let prefix = crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX;
    assert!(
        reason.starts_with(prefix),
        "the mid-stream stop reason must carry the repetition stop prefix, got {reason}"
    );
    // No hook-side warning on the Stop path (the executor surfaces it).
    assert!(
        warnings(&mut warning_rx).is_empty(),
        "the Stop path must not publish a hook-side warning"
    );

    Ok(())
}

/// Cross-turn ladder via `on_model_turn_finished`: turns 1-4 identical are
/// below threshold (each turn completes), the 5th is First →
/// `retry_with_feedback` (+ warning), 6th/7th Backoff (retry + warning), the
/// 8th Stop (the turn's stream errors with Cancelled) + no warning.
/// Rig's stream ends after one model turn, so each scripted turn is queued
/// and run separately through the SHARED hook state — exactly how the
/// executor runs production turns.
#[tokio::test]
async fn on_model_turn_finished_cross_turn_ladder() -> Result<()> {
    // -- Setup & Fixtures
    let fx = RepetitionTurnFixture::new()?;
    // Subscribe BEFORE driving any turn (see the intra-stream test).
    let mut warning_rx = fx.bus.ui_event().subscribe();

    // -- Exec & Check
    let mut total_warnings = 0usize;
    for turn in 1..=8usize {
        fx.queue_turn(vec![identical_text_turn(1)]);
        let items = fx.turn(false).await?;
        // Each `warnings()` drain is destructive; tally what this turn fired.
        let turn_warnings = warnings(&mut warning_rx);
        total_warnings += turn_warnings.len();
        match turn {
            1..=4 => {
                assert!(
                    items.iter().any(|item| matches!(
                        item,
                        Ok(rig::agent::MultiTurnStreamItem::FinalResponse(_))
                    )),
                    "turn {turn} below threshold must complete, got {items:?}"
                );
                assert!(
                    !items.iter().any(|item| item.is_err()),
                    "turn {turn} below threshold must not error, got {items:?}"
                );
                assert!(
                    turn_warnings.is_empty(),
                    "turn {turn} below threshold must not warn, got {turn_warnings:?}"
                );
            }
            5..=7 => {
                // First + Backoff x2: the turn is retried (ModelTurnRetried
                // marker) and the retry consumes the queued next turn.
                assert!(
                    items.iter().any(|item| matches!(
                        item,
                        Ok(rig::agent::MultiTurnStreamItem::ModelTurnRetried { .. })
                    )),
                    "turn {turn} must be retried (ladder escalation), got {items:?}"
                );
                assert_eq!(
                    turn_warnings.len(),
                    1,
                    "turn {turn} must publish exactly one steering warning, got {turn_warnings:?}"
                );
            }
            8 => {
                // Stop: the stream ends with a Cancelled error and NO
                // hook-side warning (the executor surfaces the reason).
                assert!(
                    turn_warnings.is_empty(),
                    "turn 8 (Stop) must not publish a hook-side warning, got {turn_warnings:?}"
                );
                let mut stopped = None;
                for item in items {
                    if let Err(err) = item {
                        stopped = Some(err);
                        break;
                    }
                }
                let error_stream = stopped.ok_or("turn 8 must stop the run (stream error)")?;
                let rig::completion::PromptError::Cancelled { reason, .. } = error_stream else {
                    return Err("turn 8 stop must surface Cancelled".into());
                };
                assert!(
                    reason
                        .starts_with(crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX),
                    "turn 8 stop reason must carry the repetition stop prefix, got {reason}"
                );
            }
            _ => unreachable!(),
        }
    }
    // Total warnings across the ladder: First + 2x Backoff steering, none for Stop.
    assert_eq!(
        total_warnings, 3,
        "exactly First + 2x Backoff warnings must be published"
    );

    Ok(())
}

/// A tool-call turn resets the cross-turn counter: 4 identical text turns + a
/// tool-call turn + text turns again → the ladder never escalates beyond
/// First (the counter was reset mid-sequence).
#[tokio::test]
async fn on_model_turn_finished_tool_call_resets() -> Result<()> {
    // -- Setup & Fixtures
    let fx = RepetitionTurnFixture::new()?;
    // Subscribe BEFORE driving any turn (see the intra-stream test).
    let mut warning_rx = fx.bus.ui_event().subscribe();

    // -- Exec & Check: 4 identical text turns are below threshold.
    for turn in 1..=4usize {
        fx.queue_turn(vec![identical_text_turn(1)]);
        let items = fx.turn(false).await?;
        assert!(
            items
                .iter()
                .any(|item| matches!(item, Ok(rig::agent::MultiTurnStreamItem::FinalResponse(_)))),
            "text turn {turn} below threshold must complete, got {items:?}"
        );
    }
    // A tool-call turn: content carries a tool call, resetting the counter.
    // The turn needs two model calls (tool call + post-result final), so the
    // agent is built WITH the mock add tool and the script carries both calls.
    fx.queue_turn(vec![
        vec![
            MockStreamEvent::tool_call("tc1", "add", serde_json::json!({"x": 1, "y": 2})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::text("tool done"),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let items = fx.turn(true).await?;
    assert!(
        !items.iter().any(|item| item.is_err()),
        "tool-call turn must not error, got {items:?}"
    );
    // 4 more identical text turns after the reset must not trip.
    for turn in 1..=4usize {
        fx.queue_turn(vec![identical_text_turn(1)]);
        let items = fx.turn(false).await?;
        assert!(
            items
                .iter()
                .any(|item| matches!(item, Ok(rig::agent::MultiTurnStreamItem::FinalResponse(_)))),
            "post-reset turn {turn} must complete (counter was reset), got {items:?}"
        );
    }
    // The ladder was never consumed (all turns continued).
    assert!(
        warnings(&mut warning_rx).is_empty(),
        "a tool-call turn resets the ladder: no detection should fire"
    );

    Ok(())
}

/// Non-repetitive turns: no warning, no retry, every turn completes.
#[tokio::test]
async fn on_model_turn_finished_non_repetitive_continues_without_warning() -> Result<()> {
    // -- Setup & Fixtures
    let texts = [
        "First response with some content.",
        "Second response, quite different.",
        "Third response, again different.",
        "Fourth response, still distinct.",
        "Fifth response, unique as well.",
        "Sixth response, yet another one.",
    ];
    let fx = RepetitionTurnFixture::new()?;
    // Subscribe BEFORE driving any turn (see the intra-stream test).
    let mut warning_rx = fx.bus.ui_event().subscribe();

    // -- Exec & Check
    for (i, text) in texts.iter().enumerate() {
        fx.queue_turn(vec![vec![
            MockStreamEvent::Text((*text).to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ]]);
        let items = fx.turn(false).await?;
        assert!(
            items.iter().all(|item| item.is_ok()),
            "distinct turn {i} must complete, got {items:?}"
        );
    }

    // -- Check
    assert!(
        warnings(&mut warning_rx).is_empty(),
        "distinct turns must not warn"
    );

    Ok(())
}

/// Intra-stream stop with a PRE-ESCALATED ladder (count 4): the cross-turn
/// ladder is driven to Stop level (turns 1-8 like
/// `on_model_turn_finished_cross_turn_ladder`); the next repetitive stream is
/// still killed MID-TURN by `on_text_delta` returning stop with the
/// `OUTPUT_REPETITION_STOP_PREFIX` reason — the same Cancelled
/// mechanism as the Esc-Esc cancel path. No hook-side warning is sent (the
/// executor surfaces the stop reason).
#[tokio::test]
async fn on_text_delta_repetition_stops_mid_stream_with_pre_escalated_ladder() -> Result<()> {
    // -- Setup & Fixtures: drive the shared ladder to Stop level.
    let fx = RepetitionTurnFixture::new()?;
    for _ in 0..8usize {
        fx.queue_turn(vec![identical_text_turn(1)]);
        let _ = fx.turn(false).await?;
    }

    // Subscribe BEFORE driving the stream — broadcast sends are not buffered
    // for later subscribers.
    let mut warning_rx = fx.bus.ui_event().subscribe();

    // -- Exec: a fresh repetitive turn must die mid-stream, not complete.
    fx.queue_turn(vec![identical_text_turn(12)]);
    let items = fx.turn(false).await?;

    // -- Check: the stream stops with a Cancelled carrying the prefix.
    let mut error_stream = None;
    for item in items {
        if let Err(err) = item {
            error_stream = Some(err);
            break;
        }
    }
    let error = error_stream.ok_or("turn at Stop level must stop the run (stream error)")?;
    let rig::completion::PromptError::Cancelled { reason, .. } = error else {
        return Err("the Stop-level stop must surface Cancelled".into());
    };
    let prefix = crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX;
    assert!(
        reason.starts_with(prefix),
        "the mid-stream stop reason must carry the repetition stop prefix, got {reason}"
    );
    // No hook-side warning on the Stop path (the executor surfaces it).
    assert!(
        warnings(&mut warning_rx).is_empty(),
        "the Stop path must not publish a hook-side warning"
    );

    Ok(())
}

/// Intra-stream at ANY ladder level stops the stream mid-stream: even with
/// the cross-turn ladder escalated (count 1), detection returns Stop and the
/// stream dies mid-turn with the `OUTPUT_REPETITION_STOP_PREFIX` reason.
#[tokio::test]
async fn on_text_delta_repetition_stops_mid_stream_at_any_ladder_level() -> Result<()> {
    // -- Setup & Fixtures: drive the shared ladder to First (count 1).
    let fx = RepetitionTurnFixture::new()?;
    // Turns 1-4 are below the cross-turn threshold and complete.
    for _ in 0..4usize {
        fx.queue_turn(vec![identical_text_turn(1)]);
        let items = fx.turn(false).await?;
        assert!(
            !items.iter().any(|item| item.is_err()),
            "fixture turns below the threshold must not error, got {items:?}"
        );
    }
    // Turn 5 is the cross-turn First escalation: the boundary retry consumes
    // a queued distinct turn and completes the run.
    fx.queue_turn(vec![
        identical_text_turn(1),
        vec![
            MockStreamEvent::Text("I changed my mind.".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let items = fx.turn(false).await?;
    assert!(
        items.iter().any(|item| matches!(
            item,
            Ok(rig::agent::MultiTurnStreamItem::ModelTurnRetried { .. })
        )),
        "fixture: the 5th turn must escalate to First, got {items:?}"
    );

    // Subscribe BEFORE driving the stream.
    let mut warning_rx = fx.bus.ui_event().subscribe();

    // -- Exec: the next repetitive turn must STILL stop mid-stream
    // (escalation_count 1 < 4 does not matter — every detection stops).
    fx.queue_turn(vec![identical_text_turn(12)]);
    let items = fx.turn(false).await?;

    // -- Check: the stream stops with a Cancelled carrying the prefix.
    let mut error_stream = None;
    for item in items {
        if let Err(err) = item {
            error_stream = Some(err);
            break;
        }
    }
    let error =
        error_stream.ok_or("a repetitive turn must stop mid-stream regardless of ladder level")?;
    let rig::completion::PromptError::Cancelled { reason, .. } = error else {
        return Err("the mid-stream stop must surface Cancelled".into());
    };
    let prefix = crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX;
    assert!(
        reason.starts_with(prefix),
        "the mid-stream stop reason must carry the repetition stop prefix, got {reason}"
    );
    // No hook-side warning on the Stop path (the executor surfaces it).
    assert!(
        warnings(&mut warning_rx).is_empty(),
        "the Stop path must not publish a hook-side warning"
    );

    Ok(())
}
