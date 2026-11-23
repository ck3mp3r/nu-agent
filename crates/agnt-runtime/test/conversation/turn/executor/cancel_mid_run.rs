//! The cancel-mid-run contract: a stop that lands while a tool call runs
//! settles the call's own outcome and closes the turn.
//!
//! Two bodies, two legs. A body that can await loses the race at the proxy, so
//! the call settles cancelled without waiting for it. A body that never yields
//! (a sync walk inside the future's poll) cannot be preempted, so the call runs
//! to completion and settles its own result. Either way the turn ends on the
//! cancelled tail.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rig::test_utils::{MockCompletionModel, MockStreamEvent};
use rig::tool::server::ToolServer;
use rig::tool::{DynamicTool, ToolOutput};

use agnt_kernel::bus::{LifecycleEvent, NarrationEvent};
use agnt_kernel::{
    InferenceRecord, NoticeKind, SessionRecord, StopCause, ToolErrorKind, ToolRecord, ToolStatus,
    TurnRecord,
};

use super::executor_test_support::*;
use super::test_utils::{
    BusEventCollector, MockResolver, TurnTestObservation, shared_model_handle, test_config,
};
use super::*;
use crate::bus::Bus;
use crate::tools::handler::builtin_tool::make_dynamic_tool;
use crate::tools::handler::grep::GrepTool;

// region:    --- Fixtures

/// The name of the tool whose body can await.
const PENDING_TOOL: &str = "pending_tool";

/// One tree line: long enough for the per-line regex work to be real.
const TREE_LINE: &str = "alpha bravo charlie delta echo";

/// Lines in the fixture tree: the walk must outlast the cancel round trip, so
/// the stop lands while the call is still reading.
const TREE_LINES: usize = 1_200_000;

/// The pattern the fixture never matches: the walk reads every line.
const NEVER_MATCHES: &str = "zz[0-9]+_qq";

/// How long after the call's started record the stop lands. Long enough that
/// the call is dispatched and the proxy is subscribed; short enough that the
/// walk is still in flight.
const CANCEL_DELAY: Duration = Duration::from_millis(40);

// endregion: --- Fixtures

// region:    --- Support

/// A registry holding the never-completing tool, plus the definition the turn
/// advertises for it.
///
/// The body pends forever, so only the turn's cancel race can end the call:
/// the record it settles with is that race's outcome.
fn pending_tool() -> Result<(
    rig::tool::server::ToolServerHandle,
    crate::types::ToolDefinition,
)> {
    let handle = ToolServer::new().run();
    handle.add_dynamic_tool(DynamicTool::new(
        crate::types::ToolName::new(PENDING_TOOL)?,
        "runs until cancelled",
        serde_json::json!({}),
        move |_args| {
            Box::pin(async move {
                std::future::pending::<()>().await;
                Ok(ToolOutput::text("done"))
            })
        },
    ));
    let definition = crate::types::ToolDefinition {
        annotations: crate::types::ToolAnnotations::default(),
        name: crate::types::ToolName::new(PENDING_TOOL)?,
        description: "runs until cancelled".to_string(),
        parameters: serde_json::json!({}),
    };
    Ok((handle, definition))
}

/// A directory holding one file the never-matching pattern must scan whole.
fn grep_tree() -> Result<(tempfile::TempDir, PathBuf)> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("tree.txt");
    let mut text = String::with_capacity(TREE_LINES * (TREE_LINE.len() + 1));
    for _ in 0..TREE_LINES {
        text.push_str(TREE_LINE);
        text.push('\n');
    }
    std::fs::write(&path, text)?;
    Ok((dir, path))
}

/// The `grep` definition the turn advertises.
fn grep_definition() -> Result<crate::types::ToolDefinition> {
    Ok(crate::types::ToolDefinition {
        annotations: crate::types::ToolAnnotations::default(),
        name: crate::types::ToolName::new("grep")?,
        description: "Search file contents".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "path": {"type": "string"}
            },
            "required": ["pattern"]
        }),
    })
}

/// The arguments the turn's `grep` call carries: the never-matching pattern over
/// the whole fixture tree.
fn grep_args(tree: &Path) -> serde_json::Value {
    serde_json::json!({
        "pattern": NEVER_MATCHES,
        "path": tree.display().to_string(),
    })
}

/// Publish the turn's cancel once the running call is under way.
///
/// The dispatch gate publishes `ToolRecord::Started` before the tool body runs,
/// so that record is the sync point; the short wait after it puts the stop
/// where ESC puts one — mid-call, after the proxy has subscribed.
fn cancel_after_start(bus: Bus, tool_name: &'static str) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut stream = bus.stream().subscribe();
        while let Ok(record) = stream.recv().await {
            if let SessionRecord::Tool(ToolRecord::Started { call }) = record
                && call.name.0 == tool_name
            {
                tokio::time::sleep(CANCEL_DELAY).await;
                let _ = bus.cancel().send(crate::bus::CancelEvent::Requested).await;
                return;
            }
        }
    })
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

/// Assert the cancelled tail every mid-run stop ends on: the stop notice on
/// both lanes, no warning, the turn's completion record and its lifecycle spine.
fn assert_cancelled_tail(events: &[TurnTestObservation]) -> Result<()> {
    assert!(
        events.iter().any(|event| matches!(
            event,
            TurnTestObservation::Record(SessionRecord::Inference(InferenceRecord::Stopped {
                cause: StopCause::UserCancelled,
                reason,
            })) if reason == TURN_STOPPED_NOTICE
        )),
        "the stop notice must ride the stopped record; got: {events:?}"
    );
    assert_eq!(
        events
            .iter()
            .filter_map(TurnTestObservation::stopped_reason)
            .filter(|reason| *reason == TURN_STOPPED_NOTICE)
            .count(),
        1,
        "the stop notice must ride the narration lane once, beside the record; got: {events:?}"
    );
    assert!(
        !events.iter().any(|event| matches!(
            event,
            TurnTestObservation::Narration(NarrationEvent::SystemMessage {
                kind: NoticeKind::Warning,
                ..
            })
        )),
        "a user cancel must not warn; got: {events:?}"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            TurnTestObservation::Record(SessionRecord::Turn(TurnRecord::Completed { .. }))
        )),
        "the cancelled turn must publish its completion record; got: {events:?}"
    );
    assert!(
        events.iter().any(|event| matches!(
            event,
            TurnTestObservation::Lifecycle(LifecycleEvent::TurnCompleted { .. })
        )),
        "the cancelled turn must publish the turn-completed spine; got: {events:?}"
    );
    Ok(())
}

/// Build the turn's input for the given prompt.
fn execute_input(prompt: String) -> ExecuteInput {
    ExecuteInput {
        prompt,
        preamble: None,
        span: nu_protocol::Span::test_data(),
        task_id: None,
    }
}

/// The row the parity pin compares: the record's kind and every content field
/// a surface can observe, with the per-run values (ids, durations) left out.
fn record_row(record: &SessionRecord) -> String {
    match record {
        SessionRecord::Inference(InferenceRecord::Started { .. }) => {
            "inference:started".to_string()
        }
        SessionRecord::Tool(ToolRecord::Started { call }) => format!(
            "tool:started name={} source={:?} arguments={:?}",
            call.name.0, call.source, call.arguments
        ),
        SessionRecord::Tool(ToolRecord::Completed { call, outcome }) => format!(
            "tool:completed name={} status={:?} kind={:?} message={:?} text={:?} payload={:?}",
            call.name.0,
            outcome.status,
            outcome.kind,
            outcome.message,
            outcome.text,
            outcome.payload
        ),
        SessionRecord::Inference(InferenceRecord::Stopped { cause, reason }) => {
            format!("inference:stopped cause={cause:?} reason={reason:?}")
        }
        SessionRecord::Turn(TurnRecord::Completed { .. }) => "turn:completed".to_string(),
        // Any record main never published for this scenario lands here, so the
        // pin fails on its kind alone.
        other => format!("unpublished:{other:?}"),
    }
}

/// The ordered record sequence the turn published, one row per record.
fn record_rows(events: &[TurnTestObservation]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            TurnTestObservation::Record(record) => Some(record_row(record)),
            _ => None,
        })
        .collect()
}

/// Drive the pending-call turn, cancelled once the call is under way.
async fn run_cancelled_pending_turn(
    session_id: &str,
) -> Result<(ExecuteOutcome, Vec<TurnTestObservation>)> {
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);
    let (handle, definition) = pending_tool()?;

    // Two scripted model turns: the cancelled turn's call, then the answer it
    // never reaches.
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", PENDING_TOOL, serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    let bus = Bus::default();
    let mut collector = BusEventCollector::subscribe(&bus);
    let mut tool_infra = default_tool_infra(bus.clone());
    tool_infra.tool_server_handle = handle;
    tool_infra.visible_tool_definitions = vec![definition];
    let model = shared_model_handle(model);

    let _canceller = cancel_after_start(bus.clone(), PENDING_TOOL);
    let mut executor = make_executor(&config, &mut memory_state, model, tool_infra);
    let result = executor
        .execute(
            execute_input(format!("run {PENDING_TOOL}")),
            MockResolver,
            Some(session_id),
        )
        .await;
    let outcome = result.map_err(|e| format!("a cancelled turn must be Ok: {e:?}"))?;
    Ok((outcome, collector.drain()))
}

// endregion: --- Support

// region:    --- Tests

/// ESC while a tool body that can await runs: the proxy's cancel race wins, so
/// the call settles cancelled without waiting for the body, and the turn spine
/// closes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_during_a_yielding_tool_settles_the_call_as_cancelled() -> Result<()> {
    // -- Setup & Fixtures & Exec
    let (outcome, events) = run_cancelled_pending_turn("test-cancel-mid-run-yielding").await?;

    // -- Check
    assert!(
        matches!(outcome, ExecuteOutcome::EarlyReturn(_)),
        "a cancelled turn returns EarlyReturn"
    );
    assert_eq!(
        tool_events(&events, PENDING_TOOL),
        vec![
            (None, None, None),
            (
                Some(ToolStatus::Cancelled),
                Some(ToolErrorKind::Cancelled),
                None
            ),
        ],
        "the cancel must settle the running call as cancelled, with no message and no second record"
    );
    assert_cancelled_tail(&events)?;
    Ok(())
}

/// ESC mid-run publishes exactly main's ordered records: the completion call's
/// start, the call's pair, the hook's stop, the turn's stop notice, and the
/// turn's completion — nothing else.
///
/// Both stop records are main's: the hook narrates the stop reason it acted on
/// (`CANCELLED_STOP_REASON`), and the executor's cancelled tail narrates the
/// turn's notice (`TURN_STOPPED_NOTICE`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_during_a_yielding_tool_publishes_main_record_sequence() -> Result<()> {
    // -- Setup & Fixtures & Exec
    let (_outcome, events) = run_cancelled_pending_turn("test-cancel-mid-run-sequence").await?;

    // -- Check
    assert_eq!(
        record_rows(&events),
        vec![
            "inference:started".to_string(),
            format!("tool:started name={PENDING_TOOL} source=Unknown arguments=\"{{}}\""),
            format!(
                "tool:completed name={PENDING_TOOL} status=Cancelled kind=Some(Cancelled) \
                 message=None text=\"tool call cancelled\" payload=None"
            ),
            format!(
                "inference:stopped cause=UserCancelled reason={:?}",
                crate::hook::chain::CANCELLED_STOP_REASON
            ),
            format!("inference:stopped cause=UserCancelled reason={TURN_STOPPED_NOTICE:?}"),
            "turn:completed".to_string(),
        ],
        "the ESC-mid-run record sequence must be main's, record for record"
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
        "the turn closes once on the lifecycle spine; got: {events:?}"
    );
    Ok(())
}

/// ESC while a tool body that cannot await runs: the sync walk cannot be
/// preempted, so the call settles its own outcome and the turn still ends on the
/// cancelled tail.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_during_a_non_yielding_tool_keeps_the_call_outcome() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-cancel-mid-run-non-yielding";
    let mut memory_state = make_memory_state(&temp_dir);
    let (_tree_dir, tree) = grep_tree()?;
    let definition = grep_definition()?;

    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "grep", grep_args(&tree)),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);

    let bus = Bus::default();
    let mut collector = BusEventCollector::subscribe(&bus);
    let mut tool_infra = default_tool_infra(bus.clone());
    tool_infra
        .tool_server_handle
        .add_dynamic_tool(make_dynamic_tool::<GrepTool>(
            definition.clone(),
            std::env::current_dir()?,
            50_000,
            bus.cancel(),
        ));
    tool_infra.visible_tool_definitions = vec![definition];
    let model = shared_model_handle(model);

    // -- Exec
    let _canceller = cancel_after_start(bus.clone(), "grep");
    let mut executor = make_executor(&config, &mut memory_state, model, tool_infra);
    let result = executor
        .execute(
            execute_input(format!("search {}", tree.display())),
            MockResolver,
            Some(session_id),
        )
        .await;
    let outcome = result.map_err(|e| format!("a cancelled turn must be Ok: {e:?}"))?;
    let events = collector.drain();

    // -- Check
    assert!(
        matches!(outcome, ExecuteOutcome::EarlyReturn(_)),
        "a cancelled turn returns EarlyReturn"
    );
    assert_eq!(
        tool_events(&events, "grep"),
        vec![(None, None, None), (Some(ToolStatus::Success), None, None)],
        "a body that never yields runs to completion and settles its own result"
    );
    assert_cancelled_tail(&events)?;
    Ok(())
}

// endregion: --- Tests
