//! Control-contract tests: the never-dropped command fan-out.

use crate::bus::{Bus, ControlCommand, ReceiveError};

use crate::test_support::Result;

/// A subscriber attached before the publish observes every command in
/// publication order, with no capacity ceiling. A dropped command is a command
/// that never ran, so the flood outlasts any bounded ring several times over.
#[tokio::test]
async fn test_control_never_dropped_under_flood() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.control().subscribe();
    const FLOOD: usize = 10_000;

    // -- Exec
    for i in 0..FLOOD {
        bus.control()
            .publish(ControlCommand::EnqueueExternalPrompt {
                task_id: format!("task-{i}"),
                prompt: format!("prompt-{i}"),
            })
            .map_err(|e| format!("control should accept with a subscriber: {e:?}"))?;
    }

    // -- Check
    for i in 0..FLOOD {
        let command = rx.recv().await.map_err(|e| format!("command {i}: {e:?}"))?;
        match command {
            ControlCommand::EnqueueExternalPrompt { task_id, prompt } => {
                assert_eq!(
                    task_id,
                    format!("task-{i}"),
                    "control must preserve publication order at {i}"
                );
                assert_eq!(
                    prompt,
                    format!("prompt-{i}"),
                    "control must preserve publication order at {i}"
                );
            }
            other => panic!("unexpected command at {i}: {other:?}"),
        }
    }
    Ok(())
}

/// Control is consumed forward, never replayed: publishing with no subscriber
/// attached reports the send failure instead of silently retaining.
#[test]
fn test_control_publish_without_subscribers_reports_error() {
    // -- Setup & Fixtures
    let bus = Bus::new();

    // -- Exec & Check
    let result = bus.control().publish(ControlCommand::CompactionRequested {
        source: crate::records::CompactionSource::Auto,
    });
    assert!(
        result.is_err(),
        "a zero-subscriber control fan-out should report the send failure"
    );
}

/// A closed subscriber is pruned and does not stop delivery to the live ones.
#[tokio::test]
async fn test_control_delivers_past_a_dropped_subscriber() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let dropped = bus.control().subscribe();
    let mut live = bus.control().subscribe();

    // -- Exec
    drop(dropped);
    bus.control()
        .publish(ControlCommand::CompactionRequested {
            source: crate::records::CompactionSource::Manual,
        })
        .map_err(|e| format!("control should accept with a live subscriber: {e:?}"))?;
    drop(bus);

    // -- Check
    let command = live
        .recv()
        .await
        .map_err(|e| format!("live subscriber command: {e:?}"))?;
    assert!(
        matches!(
            command,
            ControlCommand::CompactionRequested {
                source: crate::records::CompactionSource::Manual
            }
        ),
        "a dropped subscriber must not stop delivery to the live one"
    );
    match live.recv().await {
        Err(ReceiveError::Closed) => {}
        other => panic!("a drained, orphaned handle should report Closed, got {other:?}"),
    }
    Ok(())
}
