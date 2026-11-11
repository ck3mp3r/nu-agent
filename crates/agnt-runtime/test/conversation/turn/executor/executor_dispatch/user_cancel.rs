//! User-cancel dispatch: the generic stop notice.
//!
//! A user cancel carries no doom/repetition stop prefix, so `dispatch_result`
//! surfaces it as the generic notice on the narration's `InferenceStopped` and
//! still completes the turn on the lifecycle spine. The TUI renders that notice
//! through its existing `InferenceStopped` path.

use super::*;

/// Assert the generic stop notice rides the narration's `Stopped` and that the
/// turn still completes, with no `Warning` alongside.
///
/// The notice and the completion ride different concerns, so a drained snapshot
/// cannot order them; each is asserted on its own concern.
fn assert_stopped_notice_and_completion(events: &[TurnTestObservation]) -> Result<()> {
    assert!(
        events.iter().any(|e| matches!(
            e,
            TurnTestObservation::Narration(NarrationEvent::InferenceStopped { reason })
                if reason == TURN_STOPPED_NOTICE
        )),
        "the stop notice must ride Stopped; got: {events:?}"
    );
    assert!(
        events.iter().any(|e| matches!(
            e,
            TurnTestObservation::Lifecycle(LifecycleEvent::TurnCompleted { .. })
        )),
        "the turn must still complete; got: {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            TurnTestObservation::Narration(NarrationEvent::SystemMessage {
                kind: NoticeKind::Warning,
                ..
            })
        )),
        "a user cancel must not warn; got: {events:?}"
    );
    Ok(())
}

/// A bus cancel that never reaches rig's `Cancelled` arm carries no stop
/// reason. The turn must still tell the user it stopped: the generic notice
/// rides the narration's `InferenceStopped` and the spine still completes, and the response text
/// stays empty.
#[tokio::test]
async fn dispatch_user_cancel_without_a_reason_sends_the_notice_before_completed() -> Result<()> {
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
            None,
        )
        .await
        .map_err(|e| format!("a cancelled turn must not be an error: {e}"))?;

    // -- Check
    let ExecuteOutcome::EarlyReturn(value) = outcome else {
        return Err("a cancelled turn must early-return".into());
    };
    assert_eq!(
        crate::utils::value_ext::extract_response_text_from_value(&value),
        "",
        "a user cancel keeps the empty response text"
    );
    assert_stopped_notice_and_completion(&collector.drain())?;
    Ok(())
}

/// The hook-Terminate flavor of a user cancel carries `"Cancelled by user"` as
/// its stop reason. It is not a doom/repetition prefix, so it gets the generic
/// notice too.
#[tokio::test]
async fn dispatch_user_cancel_with_the_hook_reason_sends_the_notice_before_completed() -> Result<()>
{
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
            Ok(cancelled_result(
                Some(Vec::new()),
                Vec::new(),
                Some("Cancelled by user".to_string()),
                "",
                Some(StopCause::UserCancelled),
            )),
            "the prompt".to_string(),
            nu_protocol::Span::test_data(),
            None,
            std::time::Instant::now(),
            None,
        )
        .await
        .map_err(|e| format!("a cancelled turn must not be an error: {e}"))?;

    // -- Check
    let ExecuteOutcome::EarlyReturn(value) = outcome else {
        return Err("a cancelled turn must early-return".into());
    };
    assert_eq!(
        crate::utils::value_ext::extract_response_text_from_value(&value),
        "",
        "a user cancel keeps the empty response text"
    );
    assert_stopped_notice_and_completion(&collector.drain())?;
    Ok(())
}
