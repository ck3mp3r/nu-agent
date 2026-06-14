use super::*;

use std::cell::Cell;

use crate::agent::conversation::test_helpers::TestProgressUi;
use crate::agent::protocol::{
    compaction::CompactionTriggerSource, contracts::ProgressUi, event::UiEvent,
};
use crate::compaction::CompactionOutcome;

#[test]
fn manual_and_auto_compaction_share_single_execution_path() {
    let mut ui = TestProgressUi::default();
    let counter = Cell::new(0usize);

    let manual = execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || {
        counter.set(counter.get() + 1);
        Ok(Some(CompactionOutcome {
            summarized_count: 1,
            kept_recent_count: 1,
            summary_text: "summary".to_string(),
        }))
    });
    if let Ok(event) = &manual {
        ui.emit(event);
    }
    let auto = execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
        counter.set(counter.get() + 1);
        Ok(Some(CompactionOutcome {
            summarized_count: 1,
            kept_recent_count: 1,
            summary_text: "summary".to_string(),
        }))
    });
    if let Ok(event) = &auto {
        ui.emit(event);
    }

    assert!(manual.is_ok());
    assert!(auto.is_ok());
    assert_eq!(counter.get(), 2);
}

#[test]
fn compaction_event_emits_correct_source_metadata() {
    let mut ui = TestProgressUi::default();

    execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
        Ok(Some(CompactionOutcome {
            summarized_count: 3,
            kept_recent_count: 2,
            summary_text: "auto summary body".to_string(),
        }))
    })
    .map(|event| ui.emit(&event))
    .expect("auto event");
    execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || {
        Ok(Some(CompactionOutcome {
            summarized_count: 4,
            kept_recent_count: 1,
            summary_text: "manual summary body".to_string(),
        }))
    })
    .map(|event| ui.emit(&event))
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

#[test]
fn compaction_summary_transcript_includes_source_and_counts() {
    let mut ui = TestProgressUi::default();

    execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
        Ok(Some(CompactionOutcome {
            summarized_count: 7,
            kept_recent_count: 3,
            summary_text: "summary body for transcript".to_string(),
        }))
    })
    .map(|event| ui.emit(&event))
    .expect("event");

    assert!(ui.events.contains(&UiEvent::CompactionTriggered {
        source: "auto_threshold".to_string(),
        summarized_count: 7,
        kept_recent_count: 3,
        summary_preview: "summary body for transcript".to_string(),
        summary_body: "summary body for transcript".to_string(),
    }));
}

#[test]
fn manual_and_auto_compaction_failure_surface_is_consistent() {
    let manual = execute_compaction_event_shared(CompactionTriggerSource::SlashCompact, || {
        Err("Session compaction failed: disk full".to_string())
    });
    let auto = execute_compaction_event_shared(CompactionTriggerSource::AutoThreshold, || {
        Err("Session compaction failed: disk full".to_string())
    });

    assert_eq!(manual, auto);
}

// ========================================================================
// Concurrent compaction guard tests
// ========================================================================

#[test]
fn concurrent_compaction_guard_prevents_double_entry() {
    // When the compacting flag is already set, execute_compaction_event_shared
    // (the core logic path) should be skippable. We test the AtomicBool +
    // CompactionGuard pattern in isolation since AgentConversationRuntime
    // is too expensive to construct in unit tests.
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let compacting = Arc::new(AtomicBool::new(false));

    // Simulate first compaction acquiring the lock
    assert!(
        compacting
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok(),
        "first compaction should acquire the lock"
    );
    let _guard = super::CompactionGuard(Arc::clone(&compacting));

    // Simulate second compaction attempt -- should fail
    let second_attempt =
        compacting.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed);
    assert!(
        second_attempt.is_err(),
        "second concurrent compaction should be rejected"
    );
}

#[test]
fn compaction_guard_resets_on_completion() {
    // After the CompactionGuard is dropped, the flag should be false
    // so subsequent compactions can proceed.
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let compacting = Arc::new(AtomicBool::new(false));

    // Simulate a compaction cycle
    {
        assert!(
            compacting
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
        );
        let _guard = super::CompactionGuard(Arc::clone(&compacting));

        // Flag should be true during compaction
        assert!(
            compacting.load(Ordering::Relaxed),
            "flag should be true during compaction"
        );
    }
    // _guard dropped here

    // Flag should be reset
    assert!(
        !compacting.load(Ordering::Relaxed),
        "flag should be false after guard drop"
    );

    // Subsequent compaction should succeed
    assert!(
        compacting
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok(),
        "subsequent compaction should succeed after guard drop"
    );
}

#[test]
fn compaction_guard_resets_on_simulated_error() {
    // Even if the compaction "body" returns an error, the RAII guard
    // must reset the flag so future compactions are not permanently blocked.
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

        // Simulate compaction failure
        Err("disk full".to_string())
        // _guard dropped here despite error
    };

    assert!(result.is_err());
    assert!(
        !compacting.load(Ordering::Relaxed),
        "flag must be reset even after error"
    );
}
