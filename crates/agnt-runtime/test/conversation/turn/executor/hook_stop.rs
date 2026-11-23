//! The hook-stop contract: a hook that ends rig's agent loop closes the turn on
//! the cancelled tail, with the stop narrated on every lane.
//!
//! The stop here is the backstop a repeated tool call earns: the doom-loop
//! detector steers twice, then stops the run from `on_dispatch`. rig surfaces
//! that stop as `PromptError::Cancelled`, which is the only path the executor's
//! cancelled tail runs on without a user cancel.

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use agnt_kernel::bus::LifecycleEvent;
use agnt_kernel::{SessionRecord, StopCause, ToolErrorKind, ToolRecord, ToolStatus, TurnRecord};

use super::executor_test_support::*;
use super::test_utils::{
    BusEventCollector, MockResolver, TurnTestObservation, shared_model_handle, test_config,
};
use super::*;
use crate::bus::Bus;
use crate::hook::chain::CANCELLED_DISPATCH_MESSAGE;
use crate::hook::doom_loop::{DOOM_LOOP_BACKOFF_LIMIT, DOOM_LOOP_NOTICE, DOOM_LOOP_THRESHOLD};

// region:    --- Fixtures

/// The tool the looping model keeps calling.
const LOOPING_TOOL: &str = "echo_tool";

/// The identical calls that reach the stop detection: the first detection lands
/// on the threshold call, and each backoff call adds one more escalation.
const LOOPING_CALLS: usize = DOOM_LOOP_THRESHOLD + DOOM_LOOP_BACKOFF_LIMIT + 1;

// endregion: --- Fixtures

// region:    --- Support

/// The turn whose reply is the looping call, `LOOPING_CALLS` times over.
fn looping_reply() -> Vec<MockStreamEvent> {
    let mut turn: Vec<MockStreamEvent> = (0..LOOPING_CALLS)
        .map(|index| {
            MockStreamEvent::tool_call(format!("tc{index}"), LOOPING_TOOL, serde_json::json!({}))
        })
        .collect();
    turn.push(MockStreamEvent::final_response_with_default_usage());
    turn
}

/// The `ToolInfra` advertising the looping tool, over the bus the test observes.
fn looping_tool_infra(bus: Bus) -> Result<ToolInfra> {
    let mut tool_infra = default_tool_infra(bus);
    tool_infra
        .tool_server_handle
        .add_dynamic_tool(rig::tool::DynamicTool::new(
            crate::types::ToolName::new(LOOPING_TOOL)?,
            "echoes a fixed result",
            serde_json::json!({"type": "object", "properties": {}}),
            |_args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("echoed")) }),
        ));
    tool_infra.visible_tool_definitions = vec![crate::types::ToolDefinition {
        annotations: crate::types::ToolAnnotations::default(),
        name: crate::types::ToolName::new(LOOPING_TOOL)?,
        description: "echoes a fixed result".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    }];
    Ok(tool_infra)
}

/// One tool record's shape: `(status, error kind, message)`, with the status
/// `None` for a started row.
type ToolEvent = (Option<ToolStatus>, Option<ToolErrorKind>, Option<String>);

/// Every record the turn published for `tool_name`, in publication order.
fn tool_events(events: &[TurnTestObservation], tool_name: &str) -> Vec<ToolEvent> {
    events
        .iter()
        .filter_map(|event| match event {
            TurnTestObservation::Record(SessionRecord::Tool(ToolRecord::Started { call }))
                if call.name.0 == tool_name =>
            {
                Some((None, None, None))
            }
            TurnTestObservation::Record(SessionRecord::Tool(ToolRecord::Completed {
                call,
                outcome,
            })) if call.name.0 == tool_name => {
                Some((Some(outcome.status), outcome.kind, outcome.message.clone()))
            }
            _ => None,
        })
        .collect()
}

/// The stops the turn published on the record lane, as `(cause, reason)` rows.
fn stopped_rows(events: &[TurnTestObservation]) -> Vec<(StopCause, String)> {
    events
        .iter()
        .filter_map(|event| match event {
            TurnTestObservation::Record(SessionRecord::Inference(
                agnt_kernel::InferenceRecord::Stopped { cause, reason },
            )) => Some((*cause, reason.clone())),
            _ => None,
        })
        .collect()
}

// endregion: --- Support

// region:    --- Tests

/// WHEN the hook stops the run from `on_dispatch`, THE executor SHALL close the
/// turn on the cancelled tail: the looping call's pair settles, the stop rides
/// the record lane as `DoomLoop`, the turn publishes its completion record and
/// its lifecycle spine, and the stop reason warns on the narration lane.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hook_stop_from_dispatch_closes_the_turn_on_the_cancelled_tail() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-hook-stop-dispatch";
    let mut memory_state = make_memory_state(&temp_dir);

    let model = MockCompletionModel::from_stream_turns([
        looping_reply(),
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let shared_model = shared_model_handle(model);

    let bus = Bus::default();
    let mut collector = BusEventCollector::subscribe(&bus);
    let tool_infra = looping_tool_infra(bus.clone())?;
    let mut executor = make_executor(&config, &mut memory_state, shared_model, tool_infra);

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: format!("call {LOOPING_TOOL}"),
                preamble: None,
                span: nu_protocol::Span::test_data(),
                task_id: None,
            },
            MockResolver,
            Some(session_id),
        )
        .await;
    let outcome = result.map_err(|e| format!("a hook stop must early-return, not fail: {e:?}"))?;
    let events = collector.drain();

    // -- Check: the turn ends on the cancelled tail, not on an error.
    assert!(
        matches!(outcome, ExecuteOutcome::EarlyReturn(_)),
        "a hook stop returns EarlyReturn"
    );

    // The stop message the detector writes, carried by every lane the stop
    // touches: the hook's record, the executor's cancelled tail and the two
    // narration rows.
    let stop_message = format!(
        "{DOOM_LOOP_NOTICE} '{LOOPING_TOOL}' kept looping after repeated steering. The run was stopped."
    );

    // -- Check: every looping call settles its own pair, in dispatch order.
    // The first four run and settle `Success`; the three steered detections are
    // settled by rig as `Cancelled` with no message; the stopped detection is
    // settled by the hook's cancelled dispatch, which names its reason.
    let mut expected: Vec<ToolEvent> = Vec::new();
    for _ in 0..DOOM_LOOP_THRESHOLD - 1 {
        expected.push((None, None, None));
        expected.push((Some(ToolStatus::Success), None, None));
    }
    for _ in 0..DOOM_LOOP_BACKOFF_LIMIT + 1 {
        expected.push((None, None, None));
        expected.push((Some(ToolStatus::Cancelled), None, None));
    }
    expected.push((None, None, None));
    expected.push((
        Some(ToolStatus::Cancelled),
        Some(ToolErrorKind::Cancelled),
        Some(CANCELLED_DISPATCH_MESSAGE.to_string()),
    ));
    assert_eq!(
        tool_events(&events, LOOPING_TOOL),
        expected,
        "every looping call must settle its pair, in dispatch order"
    );

    // -- Check: the stop rides the record lane as `DoomLoop`. Two rows carry it:
    // the hook's own record, then the executor's cancelled tail — main's shape,
    // which the surface folds idempotently.
    assert_eq!(
        stopped_rows(&events),
        vec![
            (StopCause::DoomLoop, stop_message.clone()),
            (StopCause::DoomLoop, stop_message.clone()),
        ],
        "the stop must ride the record lane as DoomLoop"
    );

    // -- Check: the turn publishes its completion record and closes its spine
    // exactly once.
    assert!(
        events.iter().any(|event| matches!(
            event,
            TurnTestObservation::Record(SessionRecord::Turn(TurnRecord::Completed { .. }))
        )),
        "the stopped turn must publish its completion record; got: {events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                TurnTestObservation::Lifecycle(LifecycleEvent::TurnCompleted { .. })
            ))
            .count(),
        1,
        "the stopped turn closes its lifecycle spine once; got: {events:?}"
    );

    // -- Check: the stop warns once and narrates its reason once. A hook stop is
    // never the user-cancel notice.
    assert_eq!(
        events
            .iter()
            .filter_map(TurnTestObservation::warning_text)
            .filter(|text| *text == stop_message)
            .count(),
        1,
        "the stop reason must warn on the narration lane; got: {events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter_map(TurnTestObservation::stopped_reason)
            .collect::<Vec<&str>>(),
        vec![stop_message.as_str()],
        "the stop reason must ride the narration lane; got: {events:?}"
    );
    assert!(
        !events
            .iter()
            .filter_map(TurnTestObservation::stopped_reason)
            .any(|reason| reason == TURN_STOPPED_NOTICE),
        "a hook stop must not carry the user-cancel notice; got: {events:?}"
    );
    Ok(())
}

// endregion: --- Tests
