use super::*;

// ---------------------------------------------------------------------------
// Repetition-stop cancel path (Path C): warning ordering + Stopped event
// ---------------------------------------------------------------------------

/// A repetition stop (the 8th identical text-only completion across
/// `execute()` calls sharing one `ToolInfra`, surfaced as
/// `OUTPUT_REPETITION_STOP_PREFIX` cancel reason) must:
/// 1. emit the stop-reason Warning BEFORE `SystemEvent::TurnCompleted` (Fix 1), and
/// 2. emit `SystemEvent::InferenceStopped` — never `SystemEvent::InferenceChunk` (Fix 2).
///
/// Under the stop-to-steering flow the FIRST repetition stop converts into a
/// steering retry, so the terminal stop (with the surfacing assertions) is the
/// stop at the cap (3). The cross-turn ladder accumulates across `execute()`
/// calls (each rig stream ends after one text-only model call), so the
/// fixture is an outer loop of 8 `execute()` calls sharing the
/// `RepetitionState`; the conversion's steering retry runs INSIDE call 8 and
/// consumes the next scripted turn — the stop at the cap (terminal
/// EarlyReturn) needs its own 8 identical turns per sequence, so the loop
/// keeps driving until the EarlyReturn appears.
#[tokio::test]
async fn repetition_stop_warns_before_completed_and_emits_stopped_event() -> Result<()> {
    use crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX;

    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-repetition-stop-surface";
    let mut memory_state = make_memory_state(&temp_dir);

    let bus = crate::bus::create_bus();

    // Subscribe BEFORE the turns run — broadcast sends are not buffered for
    // later subscribers. Raw SystemEvents are read from the ui_event channel in
    // send order, which is what the ordering assertions need.
    let mut ui_event_rx = bus.ui_event().subscribe();
    let mut event_collector = super::test_utils::BusEventCollector::subscribe(&bus);

    // Generous identical scripted turn supply; the exact consumption is
    // asserted below after the flow settles.
    let stop_turn = vec![
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::final_response_with_total_tokens(1),
    ];
    let script: Vec<Vec<MockStreamEvent>> = (0..40).map(|_| stop_turn.clone()).collect();
    let model = MockCompletionModel::from_stream_turns(script);
    let shared_model = super::test_utils::shared_model_handle(model);

    // SHARED repetition state — one Arc across every `execute()` so the
    // escalation ladder accumulates across turns.
    let output_repetition = shared_repetition_state();
    let mut outcome_value = None;
    for _ in 0..8 {
        let mut executor = TurnExecutor::new(
            &config,
            &mut memory_state,
            ToolInfra {
                closure_registry: Arc::new(ClosureRegistry::default()),
                mcp_registry: Arc::new(McpToolRegistry::empty()),
                tool_server_handle: rig::tool::server::ToolServer::new().run(),
                visible_tool_definitions: vec![],
                circuit_breaker: default_circuit_breaker(),
                doom_state: default_doom_state(),
                output_repetition: output_repetition.clone(),
                repetition_guard: default_repetition_guard(),
                last_total_tokens: default_last_total_tokens(),
                turn_usage: crate::hook::TurnUsage::default(),
                session_usage: crate::hook::TurnUsage::default(),
                bus: bus.clone(),
                render_registry: ToolRenderRegistry::default(),
                cwd: std::path::PathBuf::from("/tmp"),
            },
            shared_model.clone(),
            test_compaction_config(crate::bus::create_bus()),
        );
        let result = executor
            .execute(
                ExecuteInput {
                    prompt: "hello".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
        if let Ok(TurnOutcome::EarlyReturn(value)) = result {
            outcome_value = Some(value);
            break;
        }
    }
    // Call 8 stops (Path C); the conversion retries within call 8 (the fresh
    // ladder needs its own 8 identical turns), so the terminal EarlyReturn
    // carrying the stop reason is the SECOND stop, surfaced inside call 8.

    // -- Check: the turn ended in an EarlyReturn carrying the stop reason.
    let value = outcome_value.ok_or("the repetition ladder must reach Stop and EarlyReturn")?;
    let response_text = extract_response_text_from_value(&value);
    assert!(
        response_text.starts_with(OUTPUT_REPETITION_STOP_PREFIX),
        "response text must start with OUTPUT_REPETITION_STOP_PREFIX, got: {response_text}"
    );

    // The stop-reason Warning was published (converted at the boundary).
    let events = event_collector.drain();
    assert!(
        events.iter().any(|e| matches!(e, SystemEvent::SystemMessage { kind: SystemKind::Warning, text } if text.starts_with(OUTPUT_REPETITION_STOP_PREFIX))),
        "must emit a Warning starting with OUTPUT_REPETITION_STOP_PREFIX; got: {events:?}"
    );
    // The steering notice rode every conversion on the way to the cap.
    let steering_notices = events
        .iter()
        .filter(
            |e| matches!(e, SystemEvent::SystemMessage { kind: SystemKind::Warning, text } if text == REPETITION_STEERING_NOTICE),
        )
        .count();
    assert_eq!(
        steering_notices, 3,
        "each conversion must emit the steering notice; got: {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(e, SystemEvent::InferenceStopped { reason } if reason.starts_with(OUTPUT_REPETITION_STOP_PREFIX))),
        "must emit a Stopped event starting with OUTPUT_REPETITION_STOP_PREFIX; got: {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, SystemEvent::InferenceChunk { delta, aggregated } if delta.starts_with(OUTPUT_REPETITION_STOP_PREFIX) || aggregated.starts_with(OUTPUT_REPETITION_STOP_PREFIX))),
        "the stop reason must NOT ride AssistantMessage; got: {events:?}"
    );

    // Fix 1 ordering: the cancel path publishes Warning → Stopped → Completed
    // on the ui_event channel. The channel is drained only here, so a
    // retry-heavy run can overflow the broadcast buffer; `Lagged` drops the
    // OLDEST events, so skipping it keeps the last-sent events readable.
    let mut ui_events = Vec::new();
    loop {
        match ui_event_rx.try_recv() {
            Ok(event) => ui_events.push(event),
            Err(crate::bus::TryRecvError::Lagged(_)) => continue,
            Err(_) => break,
        }
    }
    assert!(
        ui_events
            .iter()
            .any(|e| matches!(e, SystemEvent::InferenceStopped { reason } if reason.starts_with(OUTPUT_REPETITION_STOP_PREFIX))),
        "the cancel path must publish SystemEvent::InferenceStopped with the reason; got {ui_events:?}"
    );
    assert!(
        ui_events
            .iter()
            .any(|e| matches!(e, SystemEvent::TurnCompleted { .. })),
        "the cancel path must publish SystemEvent::TurnCompleted; got {ui_events:?}"
    );

    Ok(())
}

/// A repetition stop converts into a steering retry: the executor appends the
/// `OUTPUT_REPETITION_BACKOFF_MESSAGE` steering message to memory, resets the
/// escalation ladder, and re-runs the turn. A stop at the cap (3) is terminal
/// EarlyReturn with the stop reason.
///
/// The cross-turn ladder accumulates across `execute()` calls, so the fixture
/// is an outer loop of 8 `execute()` calls sharing the `RepetitionState`:
/// calls 1-4 below threshold, call 5 = First (boundary retry), calls 6-7 =
/// Backoff, call 8 = Stop (Path C). The conversion's steering retry runs
/// INSIDE call 8 and consumes the next scripted turn, which stops again —
/// repeating until the stop at the cap (the fourth stop sequence) is the
/// terminal EarlyReturn carrying the stop reason.
#[tokio::test]
async fn repetition_stop_retries_up_to_cap_then_cap_stop_is_terminal() -> Result<()> {
    use crate::hook::output_repetition::{OUTPUT_REPETITION_BACKOFF_MESSAGE, RepetitionState};

    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-repetition-stop-retry";
    let mut memory_state = make_memory_state(&temp_dir);

    let bus = crate::bus::create_bus();
    let mut event_collector = super::test_utils::BusEventCollector::subscribe(&bus);

    // Generous identical scripted turn supply; the exact consumption is
    // asserted below after the flow settles. Trace: execute() calls 1-4 run
    // turns 1-4 (below threshold). Call 5's rig stream internally retries on
    // the ladder: turn 5 (First → retry_with_feedback), turns 6-7 (Backoff),
    // turn 8 (count=4 → Stop → Cancelled) → conversion (steering +
    // reset) → retry re-escalates: turns 9-12 (Stop again → conversion 2),
    // turns 13-16 (Stop again → conversion 3), turns 17-20 (Stop at cap 3 →
    // terminal EarlyReturn). 20 model calls total: the 4 below-threshold
    // calls + 8 for the first sequence + 3 x 4 for the retry sequences.
    let stop_turn = vec![
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::final_response_with_total_tokens(1),
    ];
    let script: Vec<Vec<MockStreamEvent>> = (0..40).map(|_| stop_turn.clone()).collect();
    let model = MockCompletionModel::from_stream_turns(script);
    let probe = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let output_repetition: Arc<std::sync::Mutex<RepetitionState>> = shared_repetition_state();
    let mut outcome_value = None;
    for _ in 0..8 {
        let mut executor = TurnExecutor::new(
            &config,
            &mut memory_state,
            ToolInfra {
                closure_registry: Arc::new(ClosureRegistry::default()),
                mcp_registry: Arc::new(McpToolRegistry::empty()),
                tool_server_handle: rig::tool::server::ToolServer::new().run(),
                visible_tool_definitions: vec![],
                circuit_breaker: default_circuit_breaker(),
                doom_state: default_doom_state(),
                output_repetition: output_repetition.clone(),
                repetition_guard: default_repetition_guard(),
                last_total_tokens: default_last_total_tokens(),
                turn_usage: crate::hook::TurnUsage::default(),
                session_usage: crate::hook::TurnUsage::default(),
                bus: bus.clone(),
                render_registry: ToolRenderRegistry::default(),
                cwd: std::path::PathBuf::from("/tmp"),
            },
            shared_model.clone(),
            test_compaction_config(crate::bus::create_bus()),
        );
        let result = executor
            .execute(
                ExecuteInput {
                    prompt: "hello".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
        if let Ok(TurnOutcome::EarlyReturn(value)) = result {
            outcome_value = Some(value);
            break;
        }
    }

    // -- Check: the stop at the cap is the terminal EarlyReturn.
    let value = outcome_value.ok_or("the repetition ladder must reach Stop and EarlyReturn")?;
    let response_text = extract_response_text_from_value(&value);
    assert!(
        response_text.starts_with(crate::hook::output_repetition::OUTPUT_REPETITION_STOP_PREFIX),
        "the terminal stop response must carry the stop prefix, got: {response_text}"
    );
    // Twenty model calls: 4 below-threshold + 8 (first stop sequence: the
    // escalation ladder spans turns 5-8) + 3 x 4 for the retry sequences
    // (each conversion's fresh ladder re-escalates in exactly 4 calls).
    assert_eq!(
        probe.request_count(),
        20,
        "three conversions then the terminal stop: 4 calls per retry sequence"
    );
    // Each conversion published the steering notice: 3 notices for the 3
    // conversions; the terminal stop does not ride the notice.
    let events = event_collector.drain();
    let steering_notices = events
        .iter()
        .filter(
            |e| matches!(e, SystemEvent::SystemMessage { kind: SystemKind::Warning, text } if text == REPETITION_STEERING_NOTICE),
        )
        .count();
    assert_eq!(
        steering_notices, 3,
        "each conversion must emit the steering notice; got: {events:?}"
    );
    // The conversion delivers the steering through the retry prompt: the
    // attempt right after the first stop carries the repetition backoff message
    // above the original prompt. The carrier is sticky within the turn, so
    // every later attempt carries it too.
    let requests = probe.requests();
    let first_steered = requests
        .iter()
        .position(|request| {
            let prompt = request_prompt(request);
            prompt.starts_with(OUTPUT_REPETITION_BACKOFF_MESSAGE) && prompt.ends_with("hello")
        })
        .ok_or("the conversion must steer a retry prompt")?;
    assert!(
        first_steered > 0,
        "the steering must arrive on a retry, not the first attempt"
    );
    assert!(
        request_prompt(&requests[first_steered]).ends_with("hello"),
        "the steered prompt keeps the original prompt"
    );
    // The four below-threshold turns saved their exchanges; each of the three
    // conversions persisted its stopped attempt's delta before re-running, and
    // the terminal stopped attempt appended its delta on top. The delta is
    // anchored on the prompt, so a stopped attempt whose reported history merged
    // the store's trailing user message with the steered prompt persists that
    // merged message: the count slice starts past the merged prompt message and
    // drops it (an empty slice would fall back to the two-message placeholder
    // pair), while the anchor restores the merged message itself.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    assert_eq!(
        persisted.len(),
        36,
        "the four completed turns plus one delta per stopped attempt; got {persisted:?}"
    );
    for (i, message) in persisted.iter().take(8).enumerate() {
        let expected = if i % 2 == 0 {
            "hello"
        } else {
            "I will do the thing."
        };
        assert_eq!(
            message_text(message).as_deref(),
            Some(expected),
            "persisted[{i}] must be the completed turn's exchange; got {persisted:?}"
        );
    }
    assert!(
        message_text(&persisted[8]).is_some_and(|text| text.ends_with("hello")),
        "the first stopped attempt's delta starts from the original prompt; got {persisted:?}"
    );

    Ok(())
}

/// The stop-to-steering conversion must reset the escalation ladder: after
/// the first stop + steering + reset, the fresh repetition sequence (after a
/// distinct-content retry turn resets `consecutive`) escalates to First
/// (turn-boundary steering retry), NOT to Stop (which an unreset ladder at
/// count 4 would fire immediately).
///
/// The cross-turn ladder accumulates across `execute()` calls (each rig
/// stream ends after one text-only model call), so the fixture is an outer
/// loop of `execute()` calls sharing the `RepetitionState`. The conversion's
/// steering retry runs INSIDE `execute()` call 8 and consumes the next
/// scripted turn (distinct content → completes). Execute() call 9 then runs
/// one identical turn: with the reset it escalates First (retry → distinct
/// turn → completes); without the reset it would re-reach Stop (count 4 → 5)
/// and return terminal EarlyReturn.
#[tokio::test]
async fn repetition_stop_reset_ladder_lets_retry_sequence_restart_at_first() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-repetition-stop-reset";
    let mut memory_state = make_memory_state(&temp_dir);

    // Trace: execute() calls 1-4 run turns 1-4 (below threshold). Call 5's
    // rig stream retries on the ladder: turn 5 (First → retry), turns 6-7
    // (Backoff → retry), turn 8 (count=4 → Stop → Cancelled) →
    // conversion (steering + reset) → retry consumes turn 9 (DISTINCT
    // content → no detection → completes the rig stream). The ladder is now
    // reset (count=0, segment tracking preserved) and `last_segment` is the
    // distinct turn. Calls 6-9 run turns 10-13 (consecutive 1-4, each one
    // call per execute() — one text-only completion finalizes the rig
    // stream). Call 10 runs turn 14 (consecutive=5 → First → turn-boundary
    // retry) consuming turn 15 (distinct → completes). The key reset proof:
    // turn 14's detection is First, NOT Stop (an unreset ladder at count 4
    // would fire Stop on the first post-reset detection).
    let repeat_turn = vec![
        MockStreamEvent::Text("I will do the thing.".to_string()),
        MockStreamEvent::final_response_with_total_tokens(1),
    ];
    let distinct_turn = vec![
        MockStreamEvent::Text("I changed my mind.".to_string()),
        MockStreamEvent::final_response_with_default_usage(),
    ];
    let mut script: Vec<Vec<MockStreamEvent>> = (0..8).map(|_| repeat_turn.clone()).collect();
    script.push(distinct_turn.clone()); // stop-conversion retry completes here
    // 4 repeat turns (calls 6-9, consecutive 1-4, below threshold).
    for _ in 0..4usize {
        script.push(repeat_turn.clone());
    }
    // The 5th identical turn → First (the reset proof).
    script.push(repeat_turn.clone());
    // The First boundary retry consumes a distinct turn and completes.
    script.push(distinct_turn.clone());
    let model = MockCompletionModel::from_stream_turns(script);
    let probe = model.clone();
    let shared_model = super::test_utils::shared_model_handle(model);

    let output_repetition: Arc<std::sync::Mutex<RepetitionState>> = shared_repetition_state();

    // -- Exec & Check: drive the ladder across execute() calls; every call
    // must complete (no terminal stop) once the ladder is reset.
    for _ in 0..10 {
        let mut executor = TurnExecutor::new(
            &config,
            &mut memory_state,
            ToolInfra {
                closure_registry: Arc::new(ClosureRegistry::default()),
                mcp_registry: Arc::new(McpToolRegistry::empty()),
                tool_server_handle: rig::tool::server::ToolServer::new().run(),
                visible_tool_definitions: vec![],
                circuit_breaker: default_circuit_breaker(),
                doom_state: default_doom_state(),
                output_repetition: output_repetition.clone(),
                repetition_guard: default_repetition_guard(),
                last_total_tokens: default_last_total_tokens(),
                turn_usage: crate::hook::TurnUsage::default(),
                session_usage: crate::hook::TurnUsage::default(),
                bus: crate::bus::create_bus(),
                render_registry: ToolRenderRegistry::default(),
                cwd: std::path::PathBuf::from("/tmp"),
            },
            shared_model.clone(),
            test_compaction_config(crate::bus::create_bus()),
        );
        let result = executor
            .execute(
                ExecuteInput {
                    prompt: "hello".to_string(),
                    preamble: None,
                    span: nu_protocol::Span::test_data(),
                },
                MockResolver,
                Some(session_id),
            )
            .await;
        let outcome = result.map_err(|e| format!("each execute() must be Ok: {e:?}"))?;
        assert!(
            matches!(outcome, TurnOutcome::Completed),
            "with the ladder reset every post-reset turn must complete (no terminal stop); got {outcome:?}"
        );
    }
    // 8 turns (first sequence) + 1 conversion retry (turn 9, distinct) + 4
    // fresh turns (10-13, consecutive 1-4) + 1 more (turn 14 → First) + 1
    // boundary-retry turn (15, distinct) = 15 model calls over 10 execute()
    // calls. The reset proof: the fresh sequence's 5th detection is First
    // (count 0 → 1), NOT Stop.
    assert_eq!(
        probe.request_count(),
        15,
        "the fresh identical turn must resolve at First (retry), proving the ladder was reset"
    );
    // The conversion delivers the steering through the retry prompt: the
    // attempt right after the stop carries the repetition backoff message. The
    // carrier is sticky within the turn, so later attempts carry it too; the
    // reset proof is the request count above.
    let requests = probe.requests();
    let first_steered = requests
        .iter()
        .position(|request| {
            let prompt = request_prompt(request);
            prompt.starts_with(OUTPUT_REPETITION_BACKOFF_MESSAGE) && prompt.ends_with("hello")
        })
        .ok_or("the conversion must steer a retry prompt")?;
    assert!(
        first_steered > 0,
        "the steering must arrive on a retry, not the first attempt"
    );

    Ok(())
}
