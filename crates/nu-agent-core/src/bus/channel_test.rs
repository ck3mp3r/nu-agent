use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::channel::{BroadcastRx, BroadcastTx, Metrics, MpscRx, MpscTx, OneshotRx, OneshotTx};
use crate::bus::create_bus;
use crate::bus::events::ToolEvent;

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

// -- Broadcast

#[tokio::test]
async fn broadcast_send_and_recv_increment_metrics() -> Result<()> {
    // -- Setup & Fixtures
    let (metrics, recorded) = recording();
    let tx: BroadcastTx<u32, RecordingMetrics> = BroadcastTx::with_metrics("tool", 8, metrics);
    let mut rx: BroadcastRx<u32, RecordingMetrics> = tx.subscribe();

    // -- Exec
    tx.send(1).await?;
    let _msg = rx.recv().await?;

    // -- Check
    assert_eq!(
        *recorded.lock().unwrap(),
        vec!["send", "recv"],
        "metric increments must fire once each for send and recv"
    );
    Ok(())
}

#[tokio::test]
async fn broadcast_cloned_sender_shares_metrics() -> Result<()> {
    // -- Setup & Fixtures
    let (metrics, recorded) = recording();
    let tx: BroadcastTx<u32, RecordingMetrics> = BroadcastTx::with_metrics("tool", 8, metrics);
    let tx2 = tx.clone();
    // Keep a receiver alive so broadcast sends do not fail with NoReceiver.
    let mut _rx: BroadcastRx<u32, RecordingMetrics> = tx.subscribe();

    // -- Exec
    tx.send(1).await?;
    tx2.send(2).await?;

    // -- Check
    assert_eq!(
        *recorded.lock().unwrap(),
        vec!["send", "send"],
        "both senders must share the metrics hook"
    );
    Ok(())
}

// -- Mpsc

#[tokio::test]
async fn mpsc_send_and_recv_increment_metrics() -> Result<()> {
    // -- Setup & Fixtures
    let (metrics, recorded) = recording();
    let (tx, mut rx): (
        MpscTx<String, RecordingMetrics>,
        MpscRx<String, RecordingMetrics>,
    ) = MpscTx::channel_with_metrics("worker", 8, metrics);

    // -- Exec
    tx.send("hi".to_string()).await?;
    let _msg = rx.recv().await?;

    // -- Check
    assert_eq!(
        *recorded.lock().unwrap(),
        vec!["send", "recv"],
        "metric increments must fire once each for send and recv"
    );
    Ok(())
}

// -- Oneshot

#[tokio::test]
async fn oneshot_send_and_recv_increment_metrics() -> Result<()> {
    // -- Setup & Fixtures
    let (metrics, recorded) = recording();
    let (tx, rx): (
        OneshotTx<String, RecordingMetrics>,
        OneshotRx<String, RecordingMetrics>,
    ) = OneshotTx::channel_with_metrics("oneshot", metrics);

    // -- Exec
    tx.send("done".to_string())?;
    let _msg = rx.await?;

    // -- Check
    assert_eq!(
        *recorded.lock().unwrap(),
        vec!["send", "recv"],
        "oneshot send must count once and the completed recv once"
    );
    Ok(())
}

// -- Custom hook runs (not compiled out)

#[tokio::test]
async fn custom_non_default_hook_is_called() -> Result<()> {
    // -- Setup & Fixtures
    let counter = Arc::new(AtomicUsize::new(0));
    let hook = CountingHook(counter.clone());
    let tx: BroadcastTx<u32, CountingHook> = BroadcastTx::with_metrics("tool", 8, hook);
    let mut rx: BroadcastRx<u32, CountingHook> = tx.subscribe();

    // -- Exec
    tx.send(1).await?;
    let _msg = rx.recv().await?;

    // -- Check
    assert_eq!(
        counter.load(Ordering::SeqCst),
        2,
        "a custom hook must be called once per send and once per recv"
    );
    Ok(())
}

// -- NoMetrics default compiles and works

#[tokio::test]
async fn bus_default_uses_no_metrics() -> Result<()> {
    // -- Setup & Fixtures
    let bus = create_bus();
    let mut rx = bus.tool().subscribe();

    // -- Exec
    bus.tool()
        .send(ToolEvent::Started {
            name: "read".into(),
            source: "user".into(),
            arguments: "{}".into(),
        })
        .await?;
    let event = rx.recv().await?;

    // -- Check
    assert!(
        matches!(event, ToolEvent::Started { .. }),
        "default bus channels must still send and receive"
    );
    Ok(())
}
