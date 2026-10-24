use super::*;

/// Intra-stream: 8 identical sentences streamed within one response. The
/// aggregated text trips intra-stream detection at the 100-byte growth gate
/// (the suffix check sees 5 aligned 20-byte-period repetitions) — every
/// detection stops the stream. The executor's stop-to-steering retry
/// converts the FIRST stop: steering rides the retry prompt + ladder reset +
/// re-run. The retry consumes a scripted distinct turn and COMPLETES.
#[tokio::test]
async fn journey_output_repetition_intra_stream_trips() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-output-repetition-intra-stream");
    let tool_infra = no_tools();

    // 8 identical sentences stop the stream mid-turn; the stop-to-steering
    // retry consumes the second scripted turn (different content) and
    // completes the run.
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("I changed my mind.".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();

    // -- Exec
    let (r, events) = h.turn("do the thing", model, tool_infra).await;

    // -- Check: the retry turn completed the run.
    let outcome = r.map_err(|e| format!("intra-stream trip must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "the stop-to-steering retry must complete the run, got {outcome:?}"
    );
    // The conversion delivers the steering through the retry prompt: the
    // second request's prompt carries the backoff message above the original
    // prompt.
    let requests = spy.requests();
    assert_eq!(
        requests.len(),
        2,
        "the conversion must re-run the turn exactly once"
    );
    let retry_prompt = request_prompt(&requests[1]);
    assert!(
        retry_prompt.starts_with(OUTPUT_REPETITION_BACKOFF_MESSAGE),
        "the retry prompt must carry the repetition steering message; got: {retry_prompt}"
    );
    assert!(
        retry_prompt.ends_with("do the thing"),
        "the retry prompt must keep the original prompt; got: {retry_prompt}"
    );
    // The conversion persisted the stopped attempt's delta, then the completed
    // run saved its own exchange.
    let msgs = h.raw_messages().await?;
    assert_eq!(
        msgs.len(),
        3,
        "the stopped attempt's delta, then the completed turn's exchange; got: {msgs:?}"
    );
    // The executor publishes a warning for the conversion so the user sees
    // that steering is happening instead of only a silent retry.
    assert!(
        events.iter().any(
            |e| matches!(e, UiEvent::Warning { message } if message == REPETITION_STEERING_NOTICE)
        ),
        "the conversion retry must emit the steering notice; got: {events:?}"
    );

    Ok(())
}

/// Intra-stream: the 5th identical sentence opens the 100-byte growth gate —
/// the detection returns Stop (every detection stops); the executor converts
/// it to a steering retry which consumes a scripted distinct turn and
/// completes the run with the steering message on the retry prompt.
#[tokio::test]
async fn journey_output_repetition_intra_stream_fifth_is_first_continue() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-output-repetition-intra-stream-first");
    let tool_infra = no_tools();

    // 5 identical sentences stop the stream mid-turn; the stop-to-steering
    // retry consumes the second scripted turn (different content) and
    // completes the run.
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::Text("I will do the thing.".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("I changed my mind.".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let spy = model.clone();

    // -- Exec
    let (r, events) = h.turn("do the thing", model, tool_infra).await;

    // -- Check: the retry turn completed the run.
    let outcome = r.map_err(|e| format!("5-sentence turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "the stop-to-steering retry must complete the run, got {outcome:?}"
    );
    // The conversion delivers the steering through the retry prompt: the
    // second request's prompt carries the backoff message above the original
    // prompt.
    let requests = spy.requests();
    assert_eq!(
        requests.len(),
        2,
        "the conversion must re-run the turn exactly once"
    );
    let retry_prompt = request_prompt(&requests[1]);
    assert!(
        retry_prompt.starts_with(OUTPUT_REPETITION_BACKOFF_MESSAGE),
        "the retry prompt must carry the repetition steering message; got: {retry_prompt}"
    );
    assert!(
        retry_prompt.ends_with("do the thing"),
        "the retry prompt must keep the original prompt; got: {retry_prompt}"
    );
    // The conversion persisted the stopped attempt's delta, then the completed
    // run saved its own exchange.
    let msgs = h.raw_messages().await?;
    assert_eq!(
        msgs.len(),
        3,
        "the stopped attempt's delta, then the completed turn's exchange; got: {msgs:?}"
    );
    // The executor publishes a warning for the conversion so the user sees
    // that steering is happening instead of only a silent retry.
    assert!(
        events.iter().any(
            |e| matches!(e, UiEvent::Warning { message } if message == REPETITION_STEERING_NOTICE)
        ),
        "the conversion retry must emit the steering notice; got: {events:?}"
    );

    Ok(())
}

/// Intra-stream: 4 identical sentences streamed within one response do not
/// trip; the turn completes normally.
#[tokio::test]
async fn journey_output_repetition_intra_stream_four_does_not_trip() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-output-repetition-intra-stream-four");
    let tool_infra = no_tools();

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    // -- Exec
    let (r, _) = h.turn("do the thing", model, tool_infra).await;

    // -- Check
    let outcome = r.map_err(|e| format!("4-sentence turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "4 identical sentences must not trip intra-stream"
    );

    Ok(())
}

/// Intra-stream: alternating segments never trip, even well past the threshold.
#[tokio::test]
async fn journey_output_repetition_intra_stream_alternating_never_trips() -> Result<()> {
    // -- Setup & Fixtures
    let mut h = JourneyHarness::new("journey-output-repetition-intra-stream-alt");
    let tool_infra = no_tools();

    let model = MockCompletionModel::from_stream_turns([[
        MockStreamEvent::Text("I will do the thing. ".to_string()),
        MockStreamEvent::Text("I will do the other thing. ".to_string()),
        MockStreamEvent::Text("I will do the thing. ".to_string()),
        MockStreamEvent::Text("I will do the other thing. ".to_string()),
        MockStreamEvent::Text("I will do the thing. ".to_string()),
        MockStreamEvent::Text("I will do the other thing. ".to_string()),
        MockStreamEvent::Text("I will do the thing. ".to_string()),
        MockStreamEvent::Text("I will do the other thing. ".to_string()),
        MockStreamEvent::Text("I will do the thing. ".to_string()),
        MockStreamEvent::Text("I will do the other thing. ".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ]]);

    // -- Exec
    let (r, _) = h.turn("do the thing", model, tool_infra).await;

    // -- Check
    let outcome = r.map_err(|e| format!("alternating turn must be Ok: {e:?}"))?;
    assert!(
        matches!(outcome, TurnOutcome::Completed),
        "alternating segments must never trip intra-stream"
    );

    Ok(())
}
