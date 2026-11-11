use super::*;
use agnt_kernel::{SessionLifecycle, SessionRecord, ToolCall, ToolRecord, ToolSource};
use std::time::Duration;
use tokio::time::timeout;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// A started call carrying `name`, for the channel-shape assertions below.
fn tool_call(name: &str) -> ToolCall {
    ToolCall::new(name, ToolSource::Unknown, "{}")
}

/// A published record reaches every subscriber of its channel.
#[tokio::test]
async fn publish_reaches_subscribers() -> Result<()> {
    // -- Case 1: a cancel event reaches its subscriber
    let bus = Bus::default();
    let mut rx = bus.cancel().subscribe();
    bus.cancel()
        .send(CancelEvent::Requested)
        .await
        .map_err(|e| format!("send should succeed: {e:?}"))?;
    let received = timeout(Duration::from_millis(100), rx.recv())
        .await
        .map_err(|_| "receive should not time out")?
        .map_err(|_| "receive should succeed")?;
    assert!(matches!(received, CancelEvent::Requested));

    // -- Case 2: a tool record reaches multiple stream subscribers
    let bus = Bus::default();
    let mut rx1 = bus.stream().subscribe();
    let mut rx2 = bus.stream().subscribe();
    let _ = bus
        .stream()
        .publish(SessionRecord::Tool(ToolRecord::Started {
            call: tool_call("read"),
        }));
    let received1 = timeout(Duration::from_millis(100), rx1.recv())
        .await
        .map_err(|_| "first subscriber should not time out")?
        .map_err(|_| "first subscriber should receive")?;
    let received2 = timeout(Duration::from_millis(100), rx2.recv())
        .await
        .map_err(|_| "second subscriber should not time out")?
        .map_err(|_| "second subscriber should receive")?;
    match (received1, received2) {
        (
            SessionRecord::Tool(ToolRecord::Started { call: c1 }),
            SessionRecord::Tool(ToolRecord::Started { call: c2 }),
        ) => {
            assert_eq!(c1.name.0, "read");
            assert_eq!(c2.name.0, "read");
        }
        _ => panic!("expected a started tool record on both subscribers"),
    }

    // -- Case 3: a session row reaches its subscriber
    let bus = Bus::default();
    let mut rx = bus.stream().subscribe();
    let _ = bus
        .stream()
        .publish(SessionRecord::Session(SessionLifecycle::Started {
            session_id: agnt_kernel::SessionId::new("s1"),
            hydrated: false,
        }));
    let received = timeout(Duration::from_millis(100), rx.recv())
        .await
        .map_err(|_| "receive should not time out")?
        .map_err(|_| "receive should succeed")?;
    match received {
        SessionRecord::Session(SessionLifecycle::Started {
            session_id,
            hydrated,
        }) => {
            assert_eq!(session_id.as_str(), "s1");
            assert!(!hydrated);
        }
        _ => panic!("expected a started session record"),
    }
    Ok(())
}

/// Channels are isolated, lagged subscribers recover, and a cloned bus shares
/// the same channels.
#[tokio::test]
async fn bus_channel_semantics() -> Result<()> {
    // -- Case 1: a subscriber only receives its own channel
    let bus = Bus::default();
    let mut cancel_rx = bus.cancel().subscribe();

    // No subscriber on the stream, so the publish reports no receiver. That is
    // expected — the assertion is that the cancel subscriber never sees it.
    let _ = bus
        .stream()
        .publish(SessionRecord::Tool(ToolRecord::Started {
            call: tool_call("write"),
        }));
    let result = timeout(Duration::from_millis(50), cancel_rx.recv()).await;
    assert!(
        result.is_err(),
        "cancel subscriber must not receive tool records"
    );

    // -- Case 2: a lagged subscriber continues after draining
    let bus = Bus::default();
    let mut rx = bus.cancel().subscribe();
    for _ in 0..65 {
        bus.cancel()
            .send(CancelEvent::Requested)
            .await
            .map_err(|e| format!("send should succeed: {e:?}"))?;
    }
    let first = timeout(Duration::from_millis(100), rx.recv())
        .await
        .map_err(|_| "receive should not time out")?;
    assert!(matches!(first, Err(ChannelError::Lagged { .. })));

    // Drain the remaining buffered messages so the fresh send below is the
    // next observable event.
    while rx.try_recv().is_ok() {}

    bus.cancel()
        .send(CancelEvent::Requested)
        .await
        .map_err(|e| format!("send should succeed: {e:?}"))?;
    let next = timeout(Duration::from_millis(100), rx.recv())
        .await
        .map_err(|_| "receive should not time out")?
        .map_err(|_| "receive should succeed")?;
    assert!(matches!(next, CancelEvent::Requested));

    // -- Case 3: a cloned bus shares the same channels
    let bus = Bus::default();
    let mut rx = bus.cancel().subscribe();
    let clone = bus.clone();
    clone
        .cancel()
        .send(CancelEvent::Requested)
        .await
        .map_err(|e| format!("send on clone should succeed: {e:?}"))?;
    let received = timeout(Duration::from_millis(100), rx.recv())
        .await
        .map_err(|_| "receive should not time out")?
        .map_err(|_| "receive should succeed")?;
    assert!(matches!(received, CancelEvent::Requested));
    Ok(())
}
