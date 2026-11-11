//! Turn completion: both turn-end paths publish the turn's spine row on the
//! lifecycle concern and its one unfoldable fact — the wall clock — on the
//! record stream.
//!
//! `LifecycleEvent::TurnCompleted` carries the turn's attribution and its span;
//! every other turn figure folds from the records the stream already carried.

use super::*;
use agnt_kernel::{SessionRecord, TurnRecord};

/// The first turn-completed row in `events`, as its fields.
fn completed_fields(events: &[TurnTestObservation]) -> Option<(Option<String>, u64)> {
    events.iter().find_map(|event| match event {
        TurnTestObservation::Lifecycle(LifecycleEvent::TurnCompleted {
            task_id,
            duration_ms,
        }) => Some((task_id.clone(), *duration_ms)),
        _ => None,
    })
}

/// A turn that completes normally publishes `LifecycleEvent::TurnCompleted`
/// carrying the task that attributed it.
#[tokio::test]
async fn dispatch_completed_turn_publishes_turn_completed_with_the_task_id() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-dispatch-turn-completed";
    let mut memory_state = make_memory_state(&temp_dir);
    let bus = crate::bus::Bus::default();
    let mut collector = BusEventCollector::subscribe(&bus);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(bus),
    );

    // -- Exec
    let outcome = executor
        .dispatch_result(
            Ok(completed_result("final answer")),
            "the prompt".to_string(),
            nu_protocol::Span::test_data(),
            Some(session_id),
            std::time::Instant::now(),
            Some("task-42"),
        )
        .await
        .map_err(|e| format!("a completed turn must not be an error: {e}"))?;

    // -- Check
    assert!(matches!(outcome, ExecuteOutcome::Completed));
    let events = collector.drain();
    let (task_id, _) =
        completed_fields(&events).ok_or("a completed turn must publish TurnCompleted")?;
    assert_eq!(
        task_id.as_deref(),
        Some("task-42"),
        "the turn's attribution rides its completion; got: {events:?}"
    );
    Ok(())
}

/// A turn that ends through the early-return cancel/stop path publishes
/// `LifecycleEvent::TurnCompleted` carrying the task that attributed it.
#[tokio::test]
async fn dispatch_cancelled_turn_publishes_turn_completed_with_the_task_id() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);
    let bus = crate::bus::Bus::default();
    let mut collector = BusEventCollector::subscribe(&bus);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(bus),
    );

    // -- Exec
    let outcome = executor
        .dispatch_result(
            Ok(cancelled_result(None, Vec::new(), None, "", None)),
            "the prompt".to_string(),
            nu_protocol::Span::test_data(),
            None,
            std::time::Instant::now(),
            Some("task-43"),
        )
        .await
        .map_err(|e| format!("a cancelled turn must not be an error: {e}"))?;

    // -- Check
    assert!(matches!(outcome, ExecuteOutcome::EarlyReturn(_)));
    let events = collector.drain();
    let (task_id, _) =
        completed_fields(&events).ok_or("the early-return path must publish TurnCompleted")?;
    assert_eq!(
        task_id.as_deref(),
        Some("task-43"),
        "the early-return path carries the same attribution; got: {events:?}"
    );
    Ok(())
}

/// Both turn-end paths report the turn's wall-clock span as one value: the
/// stream's `Turn` record and the lifecycle's completion agree.
#[tokio::test]
async fn dispatch_completed_turn_reports_one_duration_on_both_concerns() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let session_id = "test-dispatch-turn-duration";
    let mut memory_state = make_memory_state(&temp_dir);
    let bus = crate::bus::Bus::default();
    let mut collector = BusEventCollector::subscribe(&bus);
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(bus),
    );

    // -- Exec
    executor
        .dispatch_result(
            Ok(completed_result("final answer")),
            "the prompt".to_string(),
            nu_protocol::Span::test_data(),
            Some(session_id),
            std::time::Instant::now(),
            None,
        )
        .await
        .map_err(|e| format!("a completed turn must not be an error: {e}"))?;

    // -- Check
    let events = collector.drain();
    let (_, lifecycle_duration) =
        completed_fields(&events).ok_or("a completed turn must publish TurnCompleted")?;
    let streamed_duration = events
        .iter()
        .find_map(|event| match event {
            TurnTestObservation::Record(SessionRecord::Turn(TurnRecord::Completed {
                duration_ms,
            })) => Some(*duration_ms),
            _ => None,
        })
        .ok_or("a completed turn must narrate one turn record")?;
    assert_eq!(
        lifecycle_duration, streamed_duration,
        "the turn's wall clock is one value published on both concerns; got: {events:?}"
    );
    Ok(())
}

/// The turn's end narrates on the record stream too: exactly one `Turn` record,
/// carrying the one fact a consumer cannot fold from the inference and tool
/// records — the turn's wall clock.
#[tokio::test]
async fn dispatch_completed_turn_narrates_one_turn_record() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);
    let bus = crate::bus::Bus::default();
    let mut records = bus.stream().subscribe();
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(bus),
    );

    // -- Exec
    executor
        .dispatch_result(
            Ok(completed_result("final answer")),
            "the prompt".to_string(),
            nu_protocol::Span::test_data(),
            Some("test-turn-record-completed"),
            std::time::Instant::now(),
            None,
        )
        .await
        .map_err(|e| format!("a completed turn must not be an error: {e}"))?;

    // -- Check
    let narrated = turn_records(&mut records);
    assert_eq!(
        narrated.len(),
        1,
        "a completed turn narrates exactly one turn record"
    );
    Ok(())
}

/// A turn that stops early still completes: its end narrates one `Turn` record,
/// on the same stream, before the surface-facing event.
#[tokio::test]
async fn dispatch_cancelled_turn_narrates_one_turn_record() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);
    let bus = crate::bus::Bus::default();
    let mut records = bus.stream().subscribe();
    let mut executor = make_executor(
        &config,
        &mut memory_state,
        super::test_utils::shared_mock_model(),
        default_tool_infra(bus),
    );

    // -- Exec
    executor
        .dispatch_result(
            Ok(cancelled_result(None, Vec::new(), None, "", None)),
            "the prompt".to_string(),
            nu_protocol::Span::test_data(),
            None,
            std::time::Instant::now(),
            None,
        )
        .await
        .map_err(|e| format!("a cancelled turn must not be an error: {e}"))?;

    // -- Check
    let narrated = turn_records(&mut records);
    assert_eq!(
        narrated.len(),
        1,
        "a stopped turn still completes, so it narrates one turn record"
    );
    Ok(())
}

/// The turn records the subscriber buffered, in publish order.
fn turn_records(
    rx: &mut agnt_kernel::bus::Subscription<agnt_kernel::SessionRecord>,
) -> Vec<agnt_kernel::TurnRecord> {
    let mut out = Vec::new();
    while let Ok(record) = rx.try_recv() {
        if let agnt_kernel::SessionRecord::Turn(turn) = record {
            out.push(turn);
        }
    }
    out
}
