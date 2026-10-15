use super::*;

/// The first detection in a turn attempt skips with a message that both
/// reports the detection fact and challenges the model to reconsider.
#[tokio::test]
async fn first_detection_skips_with_steering_message() -> TestResult<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();
    let pinned = [
        "Doom loop detected: 'read_file' called 5 times with identical arguments",
        "Are you really sure you need so many tool calls?",
        "Reconsider your approach",
    ];

    // -- Exec & Check
    let actions = drive_to_first_detection(&detector, &bus).await;
    let last = actions
        .last()
        .ok_or("should have at least one action")?
        .clone()
        .ok_or("threshold call should produce an action")?;
    let last = Some(last);
    let message = skip_reason(&last).ok_or("should be a skip denial")?;
    for text in &pinned {
        assert!(
            message.contains(text),
            "first-detection skip message must contain the pinned steering text {text:?}, got: {message}"
        );
    }
    Ok(())
}

/// The second detection in the same turn attempt is a backoff: skip with
/// stronger steering that names the looping tool.
#[tokio::test]
async fn second_detection_backoff_skips_naming_tool() -> TestResult<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();

    // -- Exec & Check
    let actions = drive_to_first_detection(&detector, &bus).await;
    assert!(
        is_skip(actions.last().ok_or("should have a last action")?),
        "first detection should skip with steering"
    );

    let second = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    let message = skip_reason(&second).ok_or("should be a skip denial")?;
    assert!(
        message.starts_with("Doom loop persisted:"),
        "backoff message must start with 'Doom loop persisted:', got: {message}"
    );
    assert!(
        message.contains("read_file"),
        "backoff message must name the looping tool, got: {message}"
    );
    assert!(
        message.contains("Change your approach"),
        "backoff message must contain the pinned steering text, got: {message}"
    );
    Ok(())
}

/// Detections 2 and 3 in the same turn attempt are backoffs (skip with
/// stronger steering); detection 4 stops the run.
#[tokio::test]
async fn fourth_detection_stops_run() -> TestResult<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();

    // -- Exec & Check
    let _ = drive_to_first_detection(&detector, &bus).await;
    let second = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    assert!(
        is_skip(&second),
        "second detection should be a backoff skip"
    );
    let third = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    assert!(is_skip(&third), "third detection should be a backoff skip");
    let fourth = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    let message = stop_reason(&fourth).ok_or("should be a stop denial")?;
    assert!(
        message.starts_with(DOOM_LOOP_STOP_PREFIX),
        "stop message must start with DOOM_LOOP_STOP_PREFIX, got: {message}"
    );
    assert!(
        message.contains("read_file"),
        "stop message must name the looping tool, got: {message}"
    );
    assert!(
        message.contains("stopped"),
        "stop message must state the run was stopped, got: {message}"
    );
    Ok(())
}

/// The stop detection returns the stop action and emits no warning on the bus
/// warning channel — the executor surfaces the stop reason exactly once.
#[tokio::test]
async fn stop_detection_emits_no_warning() -> TestResult<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();
    let mut ui_event_rx = bus.ui_event().subscribe();

    // -- Exec
    // First detection (skip + warning), then drain the warning channel.
    let _ = drive_to_first_detection(&detector, &bus).await;
    let _ = ui_event_rx.try_recv();
    // Second detection (backoff skip + warning), then drain.
    let second = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    assert!(
        is_skip(&second),
        "second detection should be a backoff skip"
    );
    let _ = ui_event_rx.try_recv();
    // Third detection (backoff skip + warning), then drain.
    let third = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    assert!(is_skip(&third), "third detection should be a backoff skip");
    let _ = ui_event_rx.try_recv();
    // Fourth detection (stop) — must emit no warning.
    let fourth = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;

    // -- Check
    assert!(
        stop_reason(&fourth).is_some(),
        "fourth detection should stop the run"
    );
    assert!(
        matches!(ui_event_rx.try_recv(), Err(crate::bus::TryRecvError::Empty)),
        "stop detection must not emit a warning on the bus ui_event channel"
    );
    Ok(())
}

/// After reset, a detection is a first detection again: skip with steering,
/// not stop.
#[tokio::test]
async fn reset_clears_escalation_counter() -> TestResult<()> {
    // -- Setup & Fixtures
    let state = Arc::new(Mutex::new(DoomLoopState::default()));
    let detector = DoomLoopDetector {
        state: Arc::clone(&state),
    };
    let bus = create_bus();

    // -- Exec & Check
    let _ = drive_to_first_detection(&detector, &bus).await;
    let second = detector
        .check_and_record("read_file", "{\"path\": \"same\"}", &bus)
        .await;
    assert!(
        is_skip(&second),
        "second detection should be a backoff skip before reset"
    );

    state.lock().map_err(|_| "should lock")?.reset();

    let actions = drive_to_first_detection(&detector, &bus).await;
    assert!(
        is_skip(actions.last().ok_or("should have a last action")?),
        "detection after reset should be a first detection again (skip)"
    );
    Ok(())
}
