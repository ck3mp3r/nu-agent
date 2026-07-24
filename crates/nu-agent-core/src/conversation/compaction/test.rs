use super::*;

use std::cell::Cell;

use crate::compaction::CompactionOutcome;
use crate::conversation::test_helpers::TestProgressUi;
use crate::protocol::{compaction::CompactionTriggerSource, contracts::ProgressUi, event::UiEvent};

#[tokio::test]
async fn manual_and_auto_compaction_share_single_execution_path() {
    let mut ui = TestProgressUi::default();
    let counter = Cell::new(0usize);

    let manual = execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || async {
        counter.set(counter.get() + 1);
        Ok(Some(CompactionOutcome {
            summarized_count: 1,
            kept_recent_count: 1,
            summary_text: "summary".to_string(),
            summary_total_tokens: None,
        }))
    })
    .await;
    if let Ok(Some((event, _))) = &manual {
        ui.emit(event);
    }
    let auto = execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || async {
        counter.set(counter.get() + 1);
        Ok(Some(CompactionOutcome {
            summarized_count: 1,
            kept_recent_count: 1,
            summary_text: "summary".to_string(),
            summary_total_tokens: None,
        }))
    })
    .await;
    if let Ok(Some((event, _))) = &auto {
        ui.emit(event);
    }

    assert!(manual.is_ok());
    assert!(auto.is_ok());
    assert_eq!(counter.get(), 2);
}

#[tokio::test]
async fn compaction_event_emits_correct_source_metadata() {
    let mut ui = TestProgressUi::default();

    execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || async {
        Ok(Some(CompactionOutcome {
            summarized_count: 3,
            kept_recent_count: 2,
            summary_text: "auto summary body".to_string(),
            summary_total_tokens: None,
        }))
    })
    .await
    .map(|opt| opt.map(|(event, _)| ui.emit(&event)))
    .expect("auto event");
    execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || async {
        Ok(Some(CompactionOutcome {
            summarized_count: 4,
            kept_recent_count: 1,
            summary_text: "manual summary body".to_string(),
            summary_total_tokens: None,
        }))
    })
    .await
    .map(|opt| opt.map(|(event, _)| ui.emit(&event)))
    .expect("manual event");

    assert!(ui.events.contains(&UiEvent::CompactionTriggered {
        source: "auto_threshold".to_string(),
        summarized_count: 3,
        kept_recent_count: 2,
        summary_preview: "auto summary body".to_string(),
        summary_body: "auto summary body".to_string(),
    }));
    assert!(ui.events.contains(&UiEvent::CompactionTriggered {
        source: "slash_compact".to_string(),
        summarized_count: 4,
        kept_recent_count: 1,
        summary_preview: "manual summary body".to_string(),
        summary_body: "manual summary body".to_string(),
    }));
}

#[tokio::test]
async fn compaction_summary_transcript_includes_source_and_counts() {
    let mut ui = TestProgressUi::default();

    execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || async {
        Ok(Some(CompactionOutcome {
            summarized_count: 7,
            kept_recent_count: 3,
            summary_text: "summary body for transcript".to_string(),
            summary_total_tokens: None,
        }))
    })
    .await
    .map(|opt| opt.map(|(event, _)| ui.emit(&event)))
    .expect("event");

    assert!(ui.events.contains(&UiEvent::CompactionTriggered {
        source: "auto_threshold".to_string(),
        summarized_count: 7,
        kept_recent_count: 3,
        summary_preview: "summary body for transcript".to_string(),
        summary_body: "summary body for transcript".to_string(),
    }));
}

#[tokio::test]
async fn manual_and_auto_compaction_failure_surface_is_consistent() {
    let manual = execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || async {
        Err("Session compaction failed: disk full".to_string())
    })
    .await;
    let auto = execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || async {
        Err("Session compaction failed: disk full".to_string())
    })
    .await;

    assert_eq!(manual, auto);
}

// ========================================================================
// Concurrent compaction guard tests
// ========================================================================

#[test]
fn concurrent_compaction_guard_prevents_double_entry() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let compacting = Arc::new(AtomicBool::new(false));

    assert!(
        compacting
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok(),
        "first compaction should acquire the lock"
    );
    let _guard = super::CompactionGuard(Arc::clone(&compacting));

    let second_attempt =
        compacting.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed);
    assert!(
        second_attempt.is_err(),
        "second concurrent compaction should be rejected"
    );
}

#[test]
fn compaction_guard_resets_on_completion() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let compacting = Arc::new(AtomicBool::new(false));

    {
        assert!(
            compacting
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        );
        let _guard = super::CompactionGuard(Arc::clone(&compacting));

        assert!(
            compacting.load(Ordering::Relaxed),
            "flag should be true during compaction"
        );
    }

    assert!(
        !compacting.load(Ordering::Relaxed),
        "flag should be false after guard drop"
    );

    assert!(
        compacting
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok(),
        "subsequent compaction should succeed after guard drop"
    );
}

#[test]
fn compaction_guard_resets_on_simulated_error() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let compacting = Arc::new(AtomicBool::new(false));

    let result: Result<(), String> = {
        assert!(
            compacting
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        );
        let _guard = super::CompactionGuard(Arc::clone(&compacting));

        Err("disk full".to_string())
    };

    assert!(result.is_err());
    assert!(
        !compacting.load(Ordering::Relaxed),
        "flag must be reset even after error"
    );
}

#[tokio::test]
async fn execute_compaction_event_shared_returns_summary_tokens() {
    let result = execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || async {
        Ok(Some(CompactionOutcome {
            summarized_count: 5,
            kept_recent_count: 3,
            summary_text: "summary".to_string(),
            summary_total_tokens: Some(5000),
        }))
    })
    .await;

    let (_, tokens) = result.expect("should succeed").expect("should be Some");
    assert_eq!(tokens, Some(5000));
}

#[tokio::test]
async fn execute_compaction_event_shared_returns_none_tokens_when_absent() {
    let result = execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || async {
        Ok(Some(CompactionOutcome {
            summarized_count: 2,
            kept_recent_count: 2,
            summary_text: "no tokens".to_string(),
            summary_total_tokens: None,
        }))
    })
    .await;

    let (_, tokens) = result.expect("should succeed").expect("should be Some");
    assert_eq!(tokens, None);
}

#[tokio::test]
async fn execute_compaction_event_shared_returns_none_when_closure_returns_none() {
    let result =
        execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || async {
            Ok(None)
        })
        .await;
    assert_eq!(result, Ok(None));
}
