//! The lag telemetry facade half: the warn line reaches the running instance's
//! own log file.
//!
//! Gated behind the `integration` feature. The test OWNS the process logger to
//! prove the facade-to-file pipe, so it must run in a process of its own — the
//! gate is what gives it one, because a gated module's tests run under nextest's
//! process-per-test. `serial_test` orders it against other serial tests and gives
//! no isolation: a shared libtest pool still lets a concurrent install take the
//! slot.

use std::io::Write;
use std::sync::Mutex;

use log::{LevelFilter, Log, Metadata, Record};

use crate::logging::{LogIdentity, LogRole, LogStore};

use super::*;

/// WHEN a hook's reader lags the record stream, THEN the warn line the runner
/// emits SHALL reach the running instance's `agent-{name}-{pid}.log` file.
#[tokio::test]
#[serial_test::serial]
async fn test_hooks_runner_lag_warning_lands_in_the_instance_log() -> Result<()> {
    // -- Setup & Fixtures: the instance log, and a logger that writes into the
    // file target the binary installs.
    let temp = tempfile::tempdir()?;
    let store = LogStore::open(temp.path().join("logs"));
    let identity = LogIdentity::new(LogRole::Run).with_name("hook-lag".to_string());
    let target = store.start(&identity);
    log::set_boxed_logger(Box::new(InstanceLogger {
        target: Mutex::new(Box::new(target)),
    }))
    .map_err(|error| format!("the process logger must install: {error}"))?;
    log::set_max_level(LevelFilter::Warn);
    let instance = LogStore::instance_path().ok_or("the process log must be installed")?;

    // -- Exec: the reader wedges inside its first invocation, then the event
    // ring cycles under it.
    let bus = Bus::default();
    let process = WedgedProcess::default();
    let hooks = vec![hook("audit", &[HookEvent::ToolStarted])];
    let handles = spawn_with(&bus, &hooks, process.clone());
    publish_tool_started(&bus, 0);
    process.entered().await;
    for index in 0..1_100 {
        let _ = bus
            .stream()
            .publish(SessionRecord::Turn(TurnRecord::Completed {
                duration_ms: index,
            }));
    }
    process.release();
    drop(bus);
    finish(handles).await?;

    // -- Check: the warn line is in the instance's own file
    let content = std::fs::read_to_string(&instance)?;
    assert!(
        content.contains("hook 'audit' lagged on the record stream"),
        "the lag line must reach {}: {content}",
        instance.display()
    );
    assert!(
        content.contains("record(s) dropped, re-synced at the tail"),
        "the lag line must name the dropped count: {content}"
    );
    Ok(())
}

// region:    --- Test Support

/// The process logger, writing every record into the instance file's target.
///
/// The binary installs `env_logger` over the same writer; the level tag plus the
/// message is the one part of its formatting this test needs.
struct InstanceLogger {
    target: Mutex<Box<dyn Write + Send>>,
}

impl Log for InstanceLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Warn
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if let Ok(mut target) = self.target.lock() {
            let _ = writeln!(target, "[{}] {}", record.level(), record.args());
            let _ = target.flush();
        }
    }

    fn flush(&self) {}
}

// endregion: --- Test Support
