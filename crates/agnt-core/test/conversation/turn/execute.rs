//! Unit tests for `drive_stream`: the grace drain's terminal outcomes.
//!
//! The scripted stream publishes the bus cancel from its first `poll_next`, so
//! the cancel is already in the channel when the loop polls `cancel_rx` — no
//! sleeps and no scheduling races. Each script then plays the outcome under
//! test: pend forever (grace expiry), `FinalResponse` then end (normal
//! completion), or `Cancelled` (history capture).

use super::*;
use crate::bus::CancelEvent;
use rig::agent::MultiTurnStreamItem;
use rig::completion::PromptError;
use rig::completion::request::Usage;
use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// One step of a [`ScriptedStream`].
enum Step {
    /// Not ready yet; the next poll plays the next step.
    Pend,
    /// Yield this item.
    Item(Box<core::result::Result<MultiTurnStreamItem, PromptError>>),
    /// End the stream.
    End,
}

/// A stream that publishes the bus cancel on its first poll, then plays
/// `steps` in order. Once the script is exhausted it pends forever.
struct ScriptedStream {
    bus: Bus,
    steps: VecDeque<Step>,
    cancel_sent: bool,
}

impl ScriptedStream {
    fn new(bus: Bus, steps: impl IntoIterator<Item = Step>) -> Self {
        Self {
            bus,
            steps: steps.into_iter().collect(),
            cancel_sent: false,
        }
    }
}

impl futures::Stream for ScriptedStream {
    type Item = core::result::Result<MultiTurnStreamItem, PromptError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.as_mut().get_mut();
        if !this.cancel_sent {
            this.cancel_sent = true;
            // The bus send is immediately ready, so one poll delivers the
            // cancel before the loop's `cancel_rx` is polled.
            let mut send = std::pin::pin!(this.bus.cancel().send(CancelEvent::Requested));
            let _ = send.as_mut().poll(cx);
        }
        match this.steps.pop_front() {
            Some(Step::Item(item)) => Poll::Ready(Some(*item)),
            Some(Step::End) => Poll::Ready(None),
            Some(Step::Pend) | None => Poll::Pending,
        }
    }
}

/// A committed tool call as the stream's `ToolCall` projection carries it.
fn streamed_call(id: &str, name: &str) -> Result<ToolCall> {
    Ok(ToolCall::from_wire(
        id,
        crate::types::ToolFunction::new(crate::types::ToolName::new(name)?, serde_json::json!({})),
    ))
}

/// A cancel followed by a stream that never yields returns at the grace
/// deadline with no history and no reason.
#[tokio::test]
async fn test_execute_drive_stream_expires_the_grace_without_history() -> Result<()> {
    // -- Setup & Fixtures
    let bus = crate::bus::create_bus();
    let stream = ScriptedStream::new(bus.clone(), [Step::Pend]);
    let grace = Duration::from_millis(50);

    // -- Exec
    let started = std::time::Instant::now();
    let result = drive_stream(stream, bus, grace, 7)
        .await
        .map_err(|e| format!("drive_stream should succeed: {e}"))?;
    let elapsed = started.elapsed();

    // -- Check
    assert!(result.cancelled, "the turn must report cancelled");
    assert!(
        result.messages.is_none(),
        "an expired grace leaves no history; got {:?}",
        result.messages
    );
    assert!(
        result.cancel_reason.is_none(),
        "a bus cancel carries no reason; got {:?}",
        result.cancel_reason
    );
    assert_eq!(
        result.pre_turn_message_count, 7,
        "the pre-turn count threads through unchanged"
    );
    assert!(
        elapsed >= grace,
        "the drain must wait out the grace; elapsed {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(1),
        "the drain must not outlast the grace bound; elapsed {elapsed:?}"
    );
    Ok(())
}

/// A cancel followed by a `FinalResponse` and a stream end completes the turn:
/// the cancel arrived too late to matter.
#[tokio::test]
async fn test_execute_drive_stream_completes_normally_during_the_grace() -> Result<()> {
    // -- Setup & Fixtures
    let bus = crate::bus::create_bus();
    let history = vec![Message::user("hi"), Message::assistant("done")];
    let response =
        rig::agent::PromptResponse::new("done", Usage::default()).with_messages(history.clone());
    let stream = ScriptedStream::new(
        bus.clone(),
        [
            Step::Pend,
            Step::Item(Box::new(Ok(MultiTurnStreamItem::FinalResponse(response)))),
            Step::End,
        ],
    );

    // -- Exec
    let result = drive_stream(stream, bus, Duration::from_millis(50), 0)
        .await
        .map_err(|e| format!("drive_stream should succeed: {e}"))?;

    // -- Check
    assert!(
        !result.cancelled,
        "a stream that ends during the grace completes the turn"
    );
    assert_eq!(
        result.messages.as_deref(),
        Some(history.as_slice()),
        "the FinalResponse history is kept"
    );
    assert_eq!(result.text, "done", "the FinalResponse output is kept");
    Ok(())
}

/// A cancel followed by `Cancelled { chat_history, reason }` captures rig's
/// canonical history and the stop reason.
#[tokio::test]
async fn test_execute_drive_stream_captures_the_cancelled_history() -> Result<()> {
    // -- Setup & Fixtures
    let bus = crate::bus::create_bus();
    let history = vec![Message::user("hi")];
    let stream = ScriptedStream::new(
        bus.clone(),
        [
            Step::Pend,
            Step::Item(Box::new(Err(PromptError::Cancelled {
                chat_history: history.clone(),
                reason: "stopped by the hook".to_string(),
            }))),
        ],
    );

    // -- Exec
    let result = drive_stream(stream, bus, Duration::from_millis(50), 0)
        .await
        .map_err(|e| format!("drive_stream should succeed: {e}"))?;

    // -- Check
    assert!(result.cancelled, "the turn must report cancelled");
    assert_eq!(
        result.messages.as_deref(),
        Some(history.as_slice()),
        "the Cancelled history is kept"
    );
    assert_eq!(
        result.cancel_reason.as_deref(),
        Some("stopped by the hook"),
        "the Cancelled reason is kept"
    );
    Ok(())
}

/// The committed-call projection is tracked in yield order, so a cancelled
/// turn carries the calls the stop left undispatched.
#[tokio::test]
async fn test_execute_drive_stream_tracks_the_committed_tool_calls() -> Result<()> {
    // -- Setup & Fixtures
    let bus = crate::bus::create_bus();
    let first = streamed_call("call_1", "read")?;
    let second = streamed_call("call_2", "write")?;
    let stream = ScriptedStream::new(
        bus.clone(),
        [
            Step::Pend,
            Step::Item(Box::new(Ok(MultiTurnStreamItem::ToolCall {
                tool_call: first.clone(),
            }))),
            Step::Item(Box::new(Ok(MultiTurnStreamItem::ToolCall {
                tool_call: second.clone(),
            }))),
            Step::Item(Box::new(Err(PromptError::Cancelled {
                chat_history: vec![Message::user("hi")],
                reason: "stopped by the hook".to_string(),
            }))),
        ],
    );

    // -- Exec
    let result = drive_stream(stream, bus, Duration::from_millis(50), 0)
        .await
        .map_err(|e| format!("drive_stream should succeed: {e}"))?;

    // -- Check
    assert!(result.cancelled, "the turn must report cancelled");
    assert_eq!(
        result.streamed_tool_calls,
        vec![first, second],
        "every committed call is tracked, in yield order"
    );
    Ok(())
}

/// A turn that commits no tool call tracks none.
#[tokio::test]
async fn test_execute_drive_stream_tracks_no_calls_without_the_projection() -> Result<()> {
    // -- Setup & Fixtures
    let bus = crate::bus::create_bus();
    let response = rig::agent::PromptResponse::new("done", Usage::default());
    let stream = ScriptedStream::new(
        bus.clone(),
        [
            Step::Pend,
            Step::Item(Box::new(Ok(MultiTurnStreamItem::FinalResponse(response)))),
            Step::End,
        ],
    );

    // -- Exec
    let result = drive_stream(stream, bus, Duration::from_millis(50), 0)
        .await
        .map_err(|e| format!("drive_stream should succeed: {e}"))?;

    // -- Check
    assert!(
        result.streamed_tool_calls.is_empty(),
        "a turn with no ToolCall item tracks no call; got {:?}",
        result.streamed_tool_calls
    );
    Ok(())
}
