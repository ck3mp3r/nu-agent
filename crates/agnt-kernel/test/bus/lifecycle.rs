//! Lifecycle-contract tests: the ordered, complete turn spine.

use std::collections::BTreeMap;

use crate::bus::{Bus, LifecycleEvent, PromptStamps};

use crate::test_support::Result;

/// The spine is observable in order with matching task ids: every external
/// prompt produces `PromptSubmitted` → `TurnStarted` → `TurnCompleted`, each
/// step carrying the id its turn is attributed by, and no step is lost even
/// under a flood.
#[tokio::test]
async fn test_lifecycle_spine_ordered_with_matching_task_ids() -> Result<()> {
    // -- Setup & Fixtures
    let bus = Bus::new();
    let mut rx = bus.lifecycle().subscribe();
    const TASKS: usize = 3;
    const ROUNDS: usize = 2_000;

    // -- Exec
    // Each round repeats the whole spine for every task: the flood outlasts any
    // bounded ring, and the per-task subsequence must stay complete and ordered.
    for round in 0..ROUNDS {
        for task in 0..TASKS {
            let task_id = Some(format!("task-{task}"));
            let stamps = PromptStamps {
                millis: Some(round as i64),
                model: Some("m".to_string()),
                agent: None,
            };
            bus.lifecycle().publish(LifecycleEvent::PromptSubmitted {
                text: format!("prompt-{round}"),
                task_id: task_id.clone(),
                stamps,
            })?;
            bus.lifecycle().publish(LifecycleEvent::TurnStarted {
                prompt: format!("prompt-{round}"),
                task_id: task_id.clone(),
            })?;
            bus.lifecycle().publish(LifecycleEvent::TurnCompleted {
                task_id,
                duration_ms: 1,
            })?;
        }
    }
    drop(bus);

    // -- Check
    let mut spine: BTreeMap<String, Vec<&'static str>> = BTreeMap::new();
    let mut received = 0usize;
    loop {
        match rx.recv().await {
            Ok(event) => {
                received += 1;
                let step = match &event {
                    LifecycleEvent::PromptSubmitted { task_id, .. } => {
                        (task_id.clone().ok_or("a spine task id")?, "submitted")
                    }
                    LifecycleEvent::TurnStarted { task_id, .. } => {
                        (task_id.clone().ok_or("a spine task id")?, "started")
                    }
                    LifecycleEvent::TurnCompleted { task_id, .. } => {
                        (task_id.clone().ok_or("a spine task id")?, "completed")
                    }
                    other => panic!("unexpected lifecycle record: {other:?}"),
                };
                spine.entry(step.0).or_default().push(step.1);
            }
            Err(error) => {
                assert!(
                    matches!(error, crate::bus::ReceiveError::Closed),
                    "lifecycle must never lag: {error:?}"
                );
                break;
            }
        }
    }
    assert_eq!(
        received,
        TASKS * ROUNDS * 3,
        "every spine step must arrive — lifecycle is complete"
    );
    for task in 0..TASKS {
        let steps = spine
            .get(&format!("task-{task}"))
            .ok_or("every task should have a spine")?;
        assert_eq!(
            steps.len(),
            ROUNDS * 3,
            "task-{task} must keep every spine step"
        );
        for round in steps.chunks(3) {
            assert_eq!(
                round,
                ["submitted", "started", "completed"],
                "the spine must be ordered submitted -> started -> completed"
            );
        }
    }
    Ok(())
}

/// Lifecycle is consumed forward: no subscriber attached means the publish
/// fails rather than silently retaining.
#[test]
fn test_lifecycle_publish_without_subscribers_reports_error() {
    // -- Setup & Fixtures
    let bus = Bus::new();

    // -- Exec & Check
    let result = bus.lifecycle().publish(LifecycleEvent::TurnCompleted {
        task_id: None,
        duration_ms: 0,
    });
    assert!(
        result.is_err(),
        "a zero-subscriber lifecycle fan-out should report the send failure"
    );
}
