use super::*;

/// A repetition stop with no session (final_session_id None) converts into a
/// steering retry that prepends the steering text to the retry prompt — no
/// session file is written and no memory append happens.
///
/// The cross-turn ladder accumulates across `execute()` calls, so the fixture
/// is an outer loop of 8 `execute()` calls sharing the `RepetitionState`; the
/// 8th stops and — with no session — the conversion prepends the steering to
/// the prompt and re-runs. The stop at the cap (3) is the terminal
/// EarlyReturn carrying the stop reason.
#[tokio::test]
async fn repetition_stop_session_less_steers_via_prompt_and_terminal_on_cap() -> Result<()> {
    use crate::hook::output_repetition::{OUTPUT_REPETITION_BACKOFF_MESSAGE, RepetitionState};

    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);

    let bus = crate::bus::create_bus();
    let mut event_collector = super::test_utils::BusEventCollector::subscribe(&bus);

    // Generous identical scripted turn supply; the exact consumption is
    // asserted below after the flow settles. Trace: execute() calls 1-4 run
    // turns 1-4 (below threshold); call 5's rig stream escalates turns 5-8
    // (First, Backoff, Backoff, Stop → Cancelled) → conversion (prompt
    // prepend + ladder reset) → each retry re-escalates in 4 calls; the stop
    // at cap 3 is terminal: 20 model calls total.
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
                None,
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
        "the session-less stop response must carry the stop prefix, got: {response_text}"
    );
    // Twenty model calls: 4 below-threshold + 8 (first stop sequence) + 3 x 4
    // retry sequences. Without the conversion the first stop would terminate
    // at 8.
    assert_eq!(
        probe.request_count(),
        20,
        "session-less retries must re-run the turn: 4 calls per retry sequence"
    );
    // The first retry's request must carry the steering as a PREPENDED prompt
    // (the transient conversation has no session memory, so the steering rides
    // the user prompt). Request 9 (index 8) is the first conversion retry: its
    // history is [steered prompt] — the steering text before the original.
    let requests = probe.requests();
    let retry_request = &requests[8];
    let history: Vec<_> = retry_request.chat_history.iter().collect();
    assert_eq!(
        history.len(),
        1,
        "the session-less retry prompt must be the steered prompt only; got: {history:?}"
    );
    let steered_prompt = message_text(history[0]).ok_or("the steered prompt must carry text")?;
    let expected = format!("{OUTPUT_REPETITION_BACKOFF_MESSAGE}\n\nhello");
    assert_eq!(
        steered_prompt, expected,
        "the retry prompt must prepend the steering text to the original prompt"
    );
    // Each conversion published the steering notice: 3 notices for the 3
    // conversions; the terminal stop does not ride the notice.
    let events = event_collector.drain();
    let steering_notices = events
        .iter()
        .filter(
            |e| matches!(e, UiEvent::Warning { message } if message == REPETITION_STEERING_NOTICE),
        )
        .count();
    assert_eq!(
        steering_notices, 3,
        "each conversion must emit the steering notice; got: {events:?}"
    );
    // Criterion 3: no persistent session file may be created by the
    // session-less retries (the backing store dir holds no .jsonl files;
    // nothing was written).
    let mut session_files = Vec::new();
    let mut read_dir = std::fs::read_dir(temp_dir.path())
        .map_err(|e| format!("tempdir read_dir should succeed: {e:?}"))?;
    while let Some(entry) = read_dir
        .next()
        .transpose()
        .map_err(|e| format!("read_dir entry: {e:?}"))?
    {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "jsonl") {
            session_files.push(path);
        }
    }
    assert!(
        session_files.is_empty(),
        "session-less steering must not write a persistent session file; got: {session_files:?}"
    );

    Ok(())
}
