//! The typed-finish contract: the reason a reply ended decides how the turn
//! fails, and that reason is read from rig's typed finish, never from the
//! diagnostic text.
//!
//! A reply the output limit cut produced no answer, so the turn cannot absorb
//! it: the failure is the output budget. The fixture is rig's own cut reply, so
//! the classification runs on what the real stream reports.

use rig::test_utils::{MockCompletionModel, MockStreamEvent};

use agnt_kernel::bus::LifecycleEvent;
use agnt_kernel::{
    InferenceRecord, SessionRecord, ToolErrorKind, ToolRecord, ToolStatus, TurnRecord,
};

use super::executor_test_support::*;
use super::test_utils::{
    BusEventCollector, MockResolver, TurnTestObservation, shared_model_handle, test_config,
    truncated_reply,
};
use super::*;
use crate::bus::Bus;
use crate::conversation::turn::TurnBudget;

// region:    --- Fixtures

/// The tool the cut reply still carries a call for.
const CUT_TOOL: &str = "echo_tool";

// endregion: --- Fixtures

// region:    --- Support

/// The `ToolInfra` advertising the tool the cut reply calls.
fn cut_tool_infra(bus: Bus) -> Result<ToolInfra> {
    let mut tool_infra = default_tool_infra(bus);
    tool_infra
        .tool_server_handle
        .add_dynamic_tool(rig::tool::DynamicTool::new(
            crate::types::ToolName::new(CUT_TOOL)?,
            "echoes a fixed result",
            serde_json::json!({"type": "object", "properties": {}}),
            |_args| Box::pin(async move { Ok(rig::tool::ToolOutput::text("echoed")) }),
        ));
    tool_infra.visible_tool_definitions = vec![crate::types::ToolDefinition {
        annotations: crate::types::ToolAnnotations::default(),
        name: crate::types::ToolName::new(CUT_TOOL)?,
        description: "echoes a fixed result".to_string(),
        parameters: serde_json::json!({"type": "object", "properties": {}}),
    }];
    Ok(tool_infra)
}

/// One tool record's shape: `(status, error kind)`, with the status `None` for a
/// started row.
type ToolEvent = (Option<ToolStatus>, Option<ToolErrorKind>);

/// Every record the turn published for `tool_name`, in publication order.
fn tool_events(events: &[TurnTestObservation], tool_name: &str) -> Vec<ToolEvent> {
    events
        .iter()
        .filter_map(|event| match event {
            TurnTestObservation::Record(SessionRecord::Tool(ToolRecord::Started { call }))
                if call.name.0 == tool_name =>
            {
                Some((None, None))
            }
            TurnTestObservation::Record(SessionRecord::Tool(ToolRecord::Completed {
                call,
                outcome,
            })) if call.name.0 == tool_name => Some((Some(outcome.status), outcome.kind)),
            _ => None,
        })
        .collect()
}

// endregion: --- Support

// region:    --- Tests

/// WHEN a reply the output limit cut fails the turn, THE executor SHALL fail it
/// on the output budget, decided by rig's typed finish reason: the cut reply's
/// tool call still settles its pair, and the failed turn closes neither the
/// turn record nor the lifecycle spine.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cut_reply_fails_the_turn_on_its_typed_finish_reason() -> Result<()> {
    // -- Setup & Fixtures: a cut reply that carries a call, then cut replies
    // until the feedback retries are exhausted.
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-finish-reason-cut-reply";
    let mut memory_state = make_memory_state(&temp_dir);

    let mut first_turn = vec![MockStreamEvent::tool_call(
        "tc1",
        CUT_TOOL,
        serde_json::json!({}),
    )];
    first_turn.push(truncated_reply());
    let model = MockCompletionModel::from_stream_turns([
        first_turn,
        vec![truncated_reply()],
        vec![truncated_reply()],
        vec![truncated_reply()],
        vec![truncated_reply()],
    ]);
    let shared_model = shared_model_handle(model);

    let bus = Bus::default();
    let mut collector = BusEventCollector::subscribe(&bus);
    let tool_infra = cut_tool_infra(bus.clone())?;
    let mut executor = make_executor(&config, &mut memory_state, shared_model, tool_infra);

    // -- Exec
    let result = executor
        .execute(
            ExecuteInput {
                prompt: format!("call {CUT_TOOL}"),
                preamble: None,
                span: nu_protocol::Span::test_data(),
                task_id: None,
            },
            MockResolver,
            Some(session_id),
        )
        .await;
    let failure = result.err().ok_or("the cut reply must fail the turn")?;
    let events = collector.drain();

    // -- Check
    assert_eq!(
        failure.budget,
        Some(TurnBudget::MaxTokens),
        "the typed finish reason must decide the budget; got: {}",
        failure.error.msg
    );
    assert_eq!(
        tool_events(&events, CUT_TOOL),
        vec![(None, None), (Some(ToolStatus::Success), None)],
        "the cut reply's call must settle its pair while the turn goes on to fail"
    );

    // -- Check: the failure leaves the spine open. The turn opened an inference,
    // and closes neither the turn record nor the lifecycle row: a failed turn
    // claims no completion.
    assert!(
        events.iter().any(|event| matches!(
            event,
            TurnTestObservation::Record(SessionRecord::Inference(InferenceRecord::Started { .. }))
        )),
        "the cut reply must open its inference call; got: {events:?}"
    );
    assert!(
        !events.iter().any(|event| matches!(
            event,
            TurnTestObservation::Record(SessionRecord::Turn(TurnRecord::Completed { .. }))
        )),
        "a failed turn must not publish a turn-completion record; got: {events:?}"
    );
    assert!(
        !events.iter().any(|event| matches!(
            event,
            TurnTestObservation::Lifecycle(LifecycleEvent::TurnCompleted { .. })
        )),
        "a failed turn must not close the lifecycle spine; got: {events:?}"
    );
    Ok(())
}

// endregion: --- Tests
