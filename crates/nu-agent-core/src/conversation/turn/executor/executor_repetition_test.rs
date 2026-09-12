//! Doom-loop and repetition-stop tests.

use std::sync::Arc;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use super::super::test::{
    default_circuit_breaker, default_doom_state, default_last_total_tokens,
    default_output_repetition, default_repetition_guard,
};
use super::REPETITION_STEERING_NOTICE;
use super::executor_test_support::*;
use super::test_utils::{MockResolver, message_text, test_compaction_config, test_config};
use super::*;
use crate::hook::doom_loop::DOOM_LOOP_STOP_PREFIX;
use crate::hook::output_repetition::{OUTPUT_REPETITION_BACKOFF_MESSAGE, RepetitionState};
use crate::protocol::event::UiEvent;
use crate::tools::closure::ClosureRegistry;
use crate::tools::handler::McpToolRegistry;
use crate::utils::value_ext::extract_response_text_from_value;

// ---------------------------------------------------------------------------
// Doom-loop stop surfacing (Path C)
// ---------------------------------------------------------------------------

/// A doom-loop stop (detection 4, the 8th identical tool call) must surface
/// the stop reason in the response text, emit a Warning, and emit an
/// AssistantMessage on the bus.
#[tokio::test]
async fn doom_stop_surfaces_reason_in_response_warning_and_assistant_message() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-doom-stop-surface";
    let mut memory_state = make_memory_state(&temp_dir);

    let tool_server_handle = rig::tool::server::ToolServer::new().run();
    tool_server_handle
        .add_dynamic_tool(rig::tool::DynamicTool::new(
            "echo_tool",
            "echoes a fixed result",
            serde_json::json!({"type": "object", "properties": {}}),
            |_context, _args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("echoed")) }),
        ))
        .await;

    // 8 identical tool calls: 5 threshold + 1 first + 2 backoff + 1 stop.
    let turns: Vec<Vec<MockStreamEvent>> = (0..8)
        .map(|i| {
            vec![
                MockStreamEvent::tool_call(format!("tc{i}"), "echo_tool", serde_json::json!({})),
                MockStreamEvent::final_response_with_default_usage(),
            ]
        })
        .collect();
    let model = MockCompletionModel::from_stream_turns(turns);
    let shared_model = super::test_utils::shared_model_handle(model);
    let bus = crate::bus::create_bus();
    let mut event_collector = super::test_utils::BusEventCollector::subscribe(&bus);

    let mut executor = TurnExecutor::new(
        &config,
        &mut memory_state,
        ToolInfra {
            closure_registry: Arc::new(ClosureRegistry::default()),
            mcp_registry: Arc::new(McpToolRegistry::empty()),
            tool_server_handle,
            visible_tool_definitions: vec![crate::types::ToolDefinition {
                name: "echo_tool".to_string(),
                description: "echoes a fixed result".to_string(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
            }],
            circuit_breaker: default_circuit_breaker(),
            doom_state: default_doom_state(),
            output_repetition: default_output_repetition(),
            repetition_guard: default_repetition_guard(),
            last_total_tokens: default_last_total_tokens(),
            bus,
        },
        shared_model,
        test_compaction_config(crate::bus::create_bus()),
    );

    // -- Exec
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

    // -- Check
    let outcome = result.map_err(|e| format!("doom stop must be Ok: {e:?}"))?;
    let TurnOutcome::EarlyReturn(value) = outcome else {
        return Err("doom stop must return EarlyReturn".into());
    };
    let response_text = extract_response_text_from_value(&value);
    assert!(
        response_text.starts_with(DOOM_LOOP_STOP_PREFIX),
        "response text must start with DOOM_LOOP_STOP_PREFIX, got: {response_text}"
    );
    assert!(
        response_text.contains("echo_tool"),
        "response text must name the looping tool, got: {response_text}"
    );

    let events = event_collector.drain();
    assert!(
        events.iter().any(|e| matches!(e, UiEvent::Warning { message } if message.starts_with(DOOM_LOOP_STOP_PREFIX))),
        "must emit a Warning starting with DOOM_LOOP_STOP_PREFIX; got: {events:?}"
    );
    // Fix 2: the stop reason rides `UiEvent::Stopped` (appended as a notice),
    // never `AssistantMessage` (which truncates the streamed block).
    assert!(
        events.iter().any(|e| matches!(e, UiEvent::Stopped { reason } if reason.starts_with(DOOM_LOOP_STOP_PREFIX))),
        "must emit a Stopped event starting with DOOM_LOOP_STOP_PREFIX; got: {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, UiEvent::AssistantMessage { text } if text.starts_with(DOOM_LOOP_STOP_PREFIX))),
        "the stop reason must NOT ride AssistantMessage; got: {events:?}"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Repetition-stop cancel path (Path C): warning ordering + Stopped event
// ---------------------------------------------------------------------------

/// A repetition stop (the 8th identical text-only completion across
/// `execute()` calls sharing one `ToolInfra`, surfaced as
/// `OUTPUT_REPETITION_STOP_PREFIX` cancel reason) must:
/// 1. emit the stop-reason Warning BEFORE `TurnEvent::Completed` (Fix 1), and
/// 2. emit `LlmEvent::Stopped` — never `LlmEvent::AssistantMessage` (Fix 2).
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
    // later subscribers. Raw LlmEvents are read from the llm channel in send
    // order, which is what the ordering assertions need.
    let mut llm_rx = bus.llm().subscribe();
    let mut turn_rx = bus.turn().subscribe();
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
                bus: bus.clone(),
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
        events.iter().any(|e| matches!(e, UiEvent::Warning { message } if message.starts_with(OUTPUT_REPETITION_STOP_PREFIX))),
        "must emit a Warning starting with OUTPUT_REPETITION_STOP_PREFIX; got: {events:?}"
    );
    // The steering notice rode every conversion on the way to the cap.
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
    assert!(
        events.iter().any(|e| matches!(e, UiEvent::Stopped { reason } if reason.starts_with(OUTPUT_REPETITION_STOP_PREFIX))),
        "must emit a Stopped event starting with OUTPUT_REPETITION_STOP_PREFIX; got: {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(e, UiEvent::AssistantMessage { text } if text.starts_with(OUTPUT_REPETITION_STOP_PREFIX))),
        "the stop reason must NOT ride AssistantMessage; got: {events:?}"
    );

    // Fix 1 ordering: the LAST TurnEvent::Completed on the turn channel is the
    // cancel path's (the executor sends Warning → Stopped → Completed in that
    // order; the warning precedes it). Every earlier turn ALSO publishes
    // Completed, so drain all of them; their mere presence plus the Warning
    // assertion above verifies the cancel path publishes both. The deterministic
    // ordering check: the warning channel's stop warning is readable NOW (it
    // was sent before this turn's Completed), which fails if finalize-clear
    // ordering were inverted.
    let mut saw_completed = false;
    while let Ok(completed) = turn_rx.try_recv() {
        assert!(
            matches!(completed, crate::bus::TurnEvent::Completed { .. }),
            "expected TurnEvent::Completed, got {completed:?}"
        );
        saw_completed = true;
    }
    assert!(saw_completed, "TurnEvent::Completed must be published");
    // The llm channel: the LAST event is the Stopped reason on the cancel turn
    // (Warning → Stopped → Completed, with Stopped replacing AssistantMessage).
    let mut llm_stopped = None;
    while let Ok(event) = llm_rx.try_recv() {
        llm_stopped = Some(event);
    }
    let llm_stopped =
        llm_stopped.ok_or("LlmEvent::Stopped must be published on the cancel path")?;
    let llm_ok = matches!(
        &llm_stopped,
        crate::bus::LlmEvent::Stopped { reason } if reason.starts_with(OUTPUT_REPETITION_STOP_PREFIX)
    );
    assert!(
        llm_ok,
        "the last llm event on the stop turn must be Stopped with the reason, got {llm_stopped:?}"
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
    // turn 8 (count=4 → Stop → PromptCancelled) → conversion (steering +
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
                bus: bus.clone(),
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
            |e| matches!(e, UiEvent::Warning { message } if message == REPETITION_STEERING_NOTICE),
        )
        .count();
    assert_eq!(
        steering_notices, 3,
        "each conversion must emit the steering notice; got: {events:?}"
    );
    // The conversion's steering message is the FIRST user message whose text
    // starts with "Output repetition" (it precedes the retry turn, whose
    // rig-side ladder feedback appends the First/Backoff detection texts
    // later). Rig's ladder feedback rides retry_with_feedback into
    // chat_history and is persisted at turn end, so a bare count would also
    // match those; ordering is the discriminator.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    let first_steering = persisted
        .iter()
        .find_map(|m| message_text(m).filter(|t| t.starts_with("Output repetition")))
        .ok_or("the conversion must append a repetition steering message")?;
    assert_eq!(
        first_steering, OUTPUT_REPETITION_BACKOFF_MESSAGE,
        "the conversion's steering message must be the repetition backoff text, got: {first_steering}"
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
    // (Backoff → retry), turn 8 (count=4 → Stop → PromptCancelled) →
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
                bus: crate::bus::create_bus(),
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
    // Exactly one BACKOFF-text repetition message: the conversion's steering.
    // The fresh sequence's first detection is First (rig appends the DETECTED
    // text as retry feedback); an unreset ladder would append a SECOND
    // BACKOFF-text message there (count 2), so the count is the discriminator.
    let persisted = load_persisted_messages(&memory_state, session_id).await?;
    let backoff_steering_count = persisted
        .iter()
        .filter(|m| message_text(m).is_some_and(|t| t == OUTPUT_REPETITION_BACKOFF_MESSAGE))
        .count();
    assert_eq!(
        backoff_steering_count, 1,
        "only the stop conversion must append a repetition steering message; got {persisted:?}"
    );

    Ok(())
}

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
    // (First, Backoff, Backoff, Stop → PromptCancelled) → conversion (prompt
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
