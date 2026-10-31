//! User-cancel dispatch: the generic stop notice.
//!
//! A user cancel carries no doom/repetition stop prefix, so `dispatch_result`
//! surfaces it as the generic notice on `SystemEvent::InferenceStopped` before
//! `SystemEvent::TurnCompleted`. The TUI renders that notice through its existing
//! `SystemEvent::InferenceStopped` path.

use super::*;

/// Assert the generic stop notice rides `Stopped` before `Completed`, and that
/// no `Warning` accompanies it.
fn assert_stopped_notice_before_completed(events: &[SystemEvent]) -> Result<()> {
    let stopped = events
        .iter()
        .position(|e| matches!(e, SystemEvent::InferenceStopped { reason } if reason == TURN_STOPPED_NOTICE))
        .ok_or_else(|| format!("the stop notice must ride Stopped; got: {events:?}"))?;
    let completed = events
        .iter()
        .position(|e| matches!(e, SystemEvent::TurnCompleted { .. }))
        .ok_or_else(|| format!("the turn must still complete; got: {events:?}"))?;
    assert!(
        stopped < completed,
        "the notice must precede Completed; got: {events:?}"
    );
    assert!(
        !events.iter().any(|e| matches!(
            e,
            SystemEvent::SystemMessage {
                kind: SystemKind::Warning,
                ..
            }
        )),
        "a user cancel must not warn; got: {events:?}"
    );
    Ok(())
}

/// A bus cancel that never reaches rig's `Cancelled` arm carries no stop
/// reason. The turn must still tell the user it stopped: the generic notice
/// rides `SystemEvent::InferenceStopped` before `Completed`, and the response text stays
/// empty.
#[tokio::test]
async fn dispatch_user_cancel_without_a_reason_sends_the_notice_before_completed() -> Result<()> {
    // -- Setup & Fixtures
    let config = test_config();
    let temp_dir = tempfile::tempdir()?;
    let mut memory_state = make_memory_state(&temp_dir);
    let bus = crate::bus::create_bus();
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
            Ok(cancelled_result(None, Vec::new(), None, "")),
            "the prompt".to_string(),
            nu_protocol::Span::test_data(),
            None,
            std::time::Instant::now(),
        )
        .await
        .map_err(|e| format!("a cancelled turn must not be an error: {e}"))?;

    // -- Check
    let TurnOutcome::EarlyReturn(value) = outcome else {
        return Err("a cancelled turn must early-return".into());
    };
    assert_eq!(
        crate::utils::value_ext::extract_response_text_from_value(&value),
        "",
        "a user cancel keeps the empty response text"
    );
    assert_stopped_notice_before_completed(&collector.drain())?;
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
    let bus = crate::bus::create_bus();
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
            )),
            "the prompt".to_string(),
            nu_protocol::Span::test_data(),
            None,
            std::time::Instant::now(),
        )
        .await
        .map_err(|e| format!("a cancelled turn must not be an error: {e}"))?;

    // -- Check
    let TurnOutcome::EarlyReturn(value) = outcome else {
        return Err("a cancelled turn must early-return".into());
    };
    assert_eq!(
        crate::utils::value_ext::extract_response_text_from_value(&value),
        "",
        "a user cancel keeps the empty response text"
    );
    assert_stopped_notice_before_completed(&collector.drain())?;
    Ok(())
}
