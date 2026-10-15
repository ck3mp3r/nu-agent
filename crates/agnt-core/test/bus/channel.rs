use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::channel::{
    BroadcastRx, BroadcastTx, Metrics, MpscRx, MpscTx, OneshotRx, OneshotTx, UiEventRx,
};
use crate::bus::create_bus;
use crate::protocol::event::UiEvent;
use crate::protocol::tool_args::CallLine;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// -- Test Support

/// A metrics hook that records the [`increment`](Metrics::increment) event
/// kinds in call order.
#[derive(Clone, Default)]
struct RecordingMetrics(Arc<Mutex<Vec<&'static str>>>);

impl Metrics for RecordingMetrics {
    fn increment(&self, name: &'static str) {
        self.0.lock().unwrap().push(name);
    }
}

fn recording() -> (RecordingMetrics, Arc<Mutex<Vec<&'static str>>>) {
    let recorded = Arc::new(Mutex::new(Vec::new()));
    (RecordingMetrics(recorded.clone()), recorded)
}

/// A metrics hook that counts increments, to prove a non-noop hook runs.
#[derive(Clone, Default)]
struct CountingHook(Arc<AtomicUsize>);

impl Metrics for CountingHook {
    fn increment(&self, _name: &'static str) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

/// Every channel backend increments the metrics hook once per send and once
/// per receive.
#[tokio::test]
async fn channel_metrics_increment_on_send_and_recv() -> Result<()> {
    // -- Case 1: broadcast
    let (metrics, recorded) = recording();
    let tx: BroadcastTx<u32, RecordingMetrics> = BroadcastTx::with_metrics("tool", 8, metrics);
    let mut rx: BroadcastRx<u32, RecordingMetrics> = tx.subscribe();
    tx.send(1).await?;
    let _msg = rx.recv().await?;
    assert_eq!(
        *recorded.lock().unwrap(),
        vec!["send", "recv"],
        "broadcast: metric increments must fire once each for send and recv"
    );

    // -- Case 2: mpsc
    let (metrics, recorded) = recording();
    let (tx, mut rx): (
        MpscTx<String, RecordingMetrics>,
        MpscRx<String, RecordingMetrics>,
    ) = MpscTx::channel_with_metrics("worker", 8, metrics);
    tx.send("hi".to_string()).await?;
    let _msg = rx.recv().await?;
    assert_eq!(
        *recorded.lock().unwrap(),
        vec!["send", "recv"],
        "mpsc: metric increments must fire once each for send and recv"
    );

    // -- Case 3: oneshot
    let (metrics, recorded) = recording();
    let (tx, rx): (
        OneshotTx<String, RecordingMetrics>,
        OneshotRx<String, RecordingMetrics>,
    ) = OneshotTx::channel_with_metrics("oneshot", metrics);
    tx.send("done".to_string())?;
    let _msg = rx.await?;
    assert_eq!(
        *recorded.lock().unwrap(),
        vec!["send", "recv"],
        "oneshot: send must count once and the completed recv once"
    );
    Ok(())
}

/// A cloned broadcast sender shares the metrics hook, and a custom non-default
/// hook is actually called.
#[tokio::test]
async fn broadcast_metrics_hook_is_shared_and_called() -> Result<()> {
    // -- Case 1: a cloned sender shares the hook
    let (metrics, recorded) = recording();
    let tx: BroadcastTx<u32, RecordingMetrics> = BroadcastTx::with_metrics("tool", 8, metrics);
    let tx2 = tx.clone();
    // Keep a receiver alive so broadcast sends do not fail with NoReceiver.
    let mut _rx: BroadcastRx<u32, RecordingMetrics> = tx.subscribe();
    tx.send(1).await?;
    tx2.send(2).await?;
    assert_eq!(
        *recorded.lock().unwrap(),
        vec!["send", "send"],
        "both senders must share the metrics hook"
    );

    // -- Case 2: a custom hook is called once per send and once per recv
    let counter = Arc::new(AtomicUsize::new(0));
    let hook = CountingHook(counter.clone());
    let tx: BroadcastTx<u32, CountingHook> = BroadcastTx::with_metrics("tool", 8, hook);
    let mut rx: BroadcastRx<u32, CountingHook> = tx.subscribe();
    tx.send(1).await?;
    let _msg = rx.recv().await?;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        2,
        "a custom hook must be called once per send and once per recv"
    );
    Ok(())
}

/// The default bus uses no metrics and its channels preserve FIFO order.
#[tokio::test]
async fn bus_default_uses_no_metrics_and_preserves_fifo() -> Result<()> {
    // -- Case 1: the default bus still sends and receives
    let bus = create_bus();
    let mut rx = bus.ui_event().subscribe();
    bus.ui_event()
        .send(UiEvent::ToolStarted {
            name: "read".into(),
            source: "user".into(),
            arguments: "{}".into(),
            call_line: CallLine::from_json_summary("{}"),
        })
        .await?;
    let event = rx.recv().await?;
    assert!(
        matches!(event, UiEvent::ToolStarted { .. }),
        "default bus channels must still send and receive"
    );

    // -- Case 2: the ui_event channel preserves FIFO order
    let bus = create_bus();
    let mut rx: UiEventRx = bus.ui_event().subscribe();
    bus.ui_event().send(UiEvent::Tick).await?;
    bus.ui_event().send(UiEvent::LlmStarted).await?;
    let first = rx.recv().await?;
    let second = rx.recv().await?;
    assert_eq!(first, UiEvent::Tick, "first event must arrive first");
    assert_eq!(
        second,
        UiEvent::LlmStarted,
        "second event must arrive second"
    );
    Ok(())
}
