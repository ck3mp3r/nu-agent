use super::*;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::timeout;

#[tokio::test]
async fn publish_cancel_reaches_subscriber() {
    let bus = create_bus();
    let mut rx = bus.cancel().subscribe();

    bus.cancel()
        .send(CancelEvent::Requested { task_id: None })
        .expect("send should succeed");

    let received = timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("receive should not time out")
        .expect("receive should succeed");

    assert!(matches!(received, CancelEvent::Requested { task_id: None }));
}

#[tokio::test]
async fn publish_tool_reaches_multiple_subscribers() {
    let bus = create_bus();
    let mut rx1 = bus.tool().subscribe();
    let mut rx2 = bus.tool().subscribe();

    bus.tool()
        .send(ToolEvent::Start {
            name: "read".into(),
            source: "user".into(),
            arguments: "{}".into(),
        })
        .expect("send should succeed");

    let received1 = timeout(Duration::from_millis(100), rx1.recv())
        .await
        .expect("first subscriber should not time out")
        .expect("first subscriber should receive");

    let received2 = timeout(Duration::from_millis(100), rx2.recv())
        .await
        .expect("second subscriber should not time out")
        .expect("second subscriber should receive");

    match (received1, received2) {
        (ToolEvent::Start { name: n1, .. }, ToolEvent::Start { name: n2, .. }) => {
            assert_eq!(n1, "read");
            assert_eq!(n2, "read");
        }
        _ => panic!("expected ToolEvent::Start on both subscribers"),
    }
}

#[tokio::test]
async fn subscriber_only_receives_its_channel() {
    let bus = create_bus();
    let mut cancel_rx = bus.cancel().subscribe();

    // No subscriber on the tool channel, so the send returns a SendError
    // (broadcast drops the message when there are zero receivers). That is
    // expected — the assertion is that the cancel subscriber never sees it.
    let _ = bus.tool().send(ToolEvent::Start {
        name: "write".into(),
        source: "system".into(),
        arguments: "{}".into(),
    });

    let result = timeout(Duration::from_millis(50), cancel_rx.recv()).await;
    assert!(
        result.is_err(),
        "cancel subscriber must not receive tool events"
    );
}

#[tokio::test]
async fn lagged_subscriber_continues() {
    let bus = create_bus();
    let mut rx = bus.cancel().subscribe();

    for _ in 0..65 {
        bus.cancel()
            .send(CancelEvent::Requested { task_id: None })
            .expect("send should succeed");
    }

    let first = timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("receive should not time out");
    assert!(matches!(first, Err(RecvError::Lagged(_))));

    // Drain the remaining buffered messages so the fresh send below is the
    // next observable event.
    while rx.try_recv().is_ok() {}

    bus.cancel()
        .send(CancelEvent::Requested {
            task_id: Some("after-lag".into()),
        })
        .expect("send should succeed");

    let next = timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("receive should not time out")
        .expect("receive should succeed");

    assert!(matches!(
        next,
        CancelEvent::Requested {
            task_id: Some(ref id)
        } if id == "after-lag"
    ));
}

#[tokio::test]
async fn clone_bus_preserves_channels() {
    let bus = create_bus();
    let mut rx = bus.cancel().subscribe();
    let clone = bus.clone();

    clone
        .cancel()
        .send(CancelEvent::Requested { task_id: None })
        .expect("send on clone should succeed");

    let received = timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("receive should not time out")
        .expect("receive should succeed");

    assert!(matches!(received, CancelEvent::Requested { task_id: None }));
}
