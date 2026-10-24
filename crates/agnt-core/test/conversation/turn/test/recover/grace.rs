//! Cancel-grace tests: a bus cancel that lands while the stream can still yield
//! keeps rig's canonical history.

use super::*;

use std::time::Duration;

/// A bus cancel that lands while the stream can still yield keeps rig's
/// canonical history: the grace drain captures `Cancelled { chat_history }`
/// instead of dropping the stream at the cancel signal.
#[tokio::test]
async fn cancel_grace_captures_the_cancelled_history() -> Result<()> {
    // -- Setup & Fixtures
    let model = MockCompletionModel::from_stream_turns([
        vec![
            MockStreamEvent::tool_call("tc1", "test_cancel_tool", serde_json::json!({})),
            MockStreamEvent::final_response_with_default_usage(),
        ],
        vec![
            MockStreamEvent::Text("unreachable".to_string()),
            MockStreamEvent::final_response_with_default_usage(),
        ],
    ]);
    let config = Config::default();
    let bus = crate::bus::create_bus();
    let ctx = make_turn_context(shared_handle(model), &config, bus.clone())
        .with_cancel_grace(Duration::from_millis(50));
    let ctx = with_tool(
        ctx,
        &config,
        bus.clone(),
        executor::test_utils::CancellingTool::new(bus),
        "test_cancel_tool",
    )?;

    // -- Exec
    let started = std::time::Instant::now();
    let result = execute_turn(ctx, MockResolver(PermissionDecision::Allow))
        .await
        .map_err(|e| format!("execute_turn should not error: {e:?}"))?;
    let elapsed = started.elapsed();

    // -- Check
    assert!(result.cancelled, "the turn must report cancelled");
    let messages = result
        .messages
        .ok_or("the grace drain must capture rig's Cancelled history")?;
    assert!(
        messages
            .iter()
            .any(|message| matches!(message, Message::User { .. })),
        "the captured history carries the tool result; got {messages:?}"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "the drain must not outlast the grace bound; elapsed {elapsed:?}"
    );
    Ok(())
}
