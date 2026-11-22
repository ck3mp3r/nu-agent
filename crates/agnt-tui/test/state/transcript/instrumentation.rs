//! The store's two per-arm cost counters: the displaced-entry re-measure and
//! the diff gate's re-projection. Both run inside a measure or a gate read, so
//! the stream arm reports their deltas rather than a fold step's.

use super::*;

use crate::runtime::RuntimeCoordinator;
use agnt_render::transcript::entry::{MessageEntry, TranscriptEntry};
use agnt_render::transcript::ir::MessageRole;

type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

/// One user message entry, as the store's push API takes it.
fn user_entry(text: &str) -> TranscriptEntry {
    TranscriptEntry::Message(MessageEntry {
        role: MessageRole::User,
        markdown: text.to_string(),
        timestamp_millis: None,
        model: None,
        agent: None,
    })
}

/// A store with `count` measured entries at width 40.
fn measured_store(count: usize) -> TranscriptStore {
    let mut store = TranscriptStore::default();
    for index in 0..count {
        store.push_entry(user_entry(&format!("entry {index}")));
    }
    store.ensure_height_index(40);
    store
}

/// WHEN a fold displaces one pending entry by marking another, THE fold SHALL
/// measure nothing, and THE frame's single flush SHALL measure both once.
#[test]
fn test_transcript_store_displaced_mark_defers_and_the_frame_pays_once() -> Result<()> {
    // -- Setup & Fixtures
    let mut store = measured_store(2);
    let (_, calls_before) = store.remeasure_cost();

    // -- Exec: the streamed entry is pending, then a second record marks the
    // pushed one — the repro's `permission` fold.
    store.mark_stale(0);
    store.mark_stale(1);
    let (_, calls_in_fold) = store.remeasure_cost();

    // -- Check: the fold paid nothing.
    assert_eq!(
        calls_in_fold, calls_before,
        "a displacement must not measure inside the fold"
    );

    // -- Exec: the frame reads the index.
    store.ensure_height_index(40);

    // -- Check: both pending entries were measured, once each.
    assert_eq!(
        store.remeasure_cost().1.saturating_sub(calls_before),
        2,
        "the frame measures every pending entry once"
    );
    Ok(())
}

/// WHEN the same entry is marked twice, THE frame SHALL measure it once.
#[test]
fn test_transcript_store_pending_marks_dedupe() -> Result<()> {
    // -- Setup & Fixtures
    let mut store = measured_store(2);
    let (_, calls_before) = store.remeasure_cost();

    // -- Exec
    store.mark_stale(0);
    store.mark_stale(0);
    store.ensure_height_index(40);

    // -- Check
    assert_eq!(
        store.remeasure_cost().1.saturating_sub(calls_before),
        1,
        "a repeated mark holds the entry once"
    );
    Ok(())
}

/// WHEN more entries are marked pending than the set may hold, THE oldest SHALL
/// be flushed in-fold — the degraded valve — and THE set SHALL stay bounded.
#[test]
fn test_transcript_store_pending_set_overflows_at_the_cap() -> Result<()> {
    // -- Setup & Fixtures: ten measured entries, nine marks to overflow.
    let mut store = measured_store(10);
    let (_, calls_before) = store.remeasure_cost();

    // -- Exec: nine distinct marks against a cap of eight.
    for index in 0..9 {
        store.mark_stale(index);
    }
    let overflow_calls = store.remeasure_cost().1.saturating_sub(calls_before);

    // -- Check: exactly one in-fold flush.
    assert_eq!(
        overflow_calls, 1,
        "one mark past the cap flushes exactly one entry in-fold"
    );

    // -- Exec: the frame drains the rest.
    store.ensure_height_index(40);

    // -- Check: nine marks, nine measures — one in-fold, eight at the frame.
    assert_eq!(
        store.remeasure_cost().1.saturating_sub(calls_before),
        9,
        "every marked entry is measured exactly once"
    );
    Ok(())
}

/// WHEN entries are evicted, THE pending set SHALL drop the marks whose entries
/// are gone and shift the survivors onto the entries they still name.
#[test]
fn test_transcript_store_pending_marks_survive_an_eviction_shift() -> Result<()> {
    // -- Setup & Fixtures: three measured entries, marks on 0 and 2.
    let mut store = measured_store(3);
    let (_, calls_before) = store.remeasure_cost();
    store.mark_stale(0);
    store.mark_stale(2);

    // -- Exec: one entry evicted from the front; entry 2 is now entry 1.
    store.shift_indices_after_eviction(1);
    store.ensure_height_index(40);

    // -- Check: the evicted mark is gone, the survivor measured.
    assert_eq!(
        store.remeasure_cost().1.saturating_sub(calls_before),
        1,
        "only the surviving mark reaches the frame"
    );
    Ok(())
}

/// WHEN entries are truncated below a pending mark, THE mark SHALL be dropped.
#[test]
fn test_transcript_store_pending_mark_past_a_truncate_is_dropped() -> Result<()> {
    // -- Setup & Fixtures
    let mut store = measured_store(3);
    let (_, calls_before) = store.remeasure_cost();
    store.mark_stale(2);

    // -- Exec: drop the entry the mark names.
    store.truncate(2);
    store.ensure_height_index(40);

    // -- Check: nothing was measured for a dead index.
    assert_eq!(
        store.remeasure_cost().1,
        calls_before,
        "a flush never targets a stale index"
    );
    Ok(())
}

/// WHEN an in-place entry is marked pending, THE store SHALL keep its previous
/// measurement until the frame flush: the approximation, never a collapse.
#[test]
fn test_transcript_store_pending_window_keeps_the_stale_approximation() -> Result<()> {
    // -- Setup & Fixtures: one measured entry, then a much longer in-place body.
    let mut store = measured_store(1);
    let rows_before = store.total_visual_rows();
    store.entries_mut()[0] = user_entry(&"line\n".repeat(40));

    // -- Exec: the fold marks it stale and measures nothing.
    store.mark_stale(0);
    let rows_pending = store.total_visual_rows();

    // -- Check: the stale approximation still stands.
    assert_eq!(
        rows_pending, rows_before,
        "a pending entry keeps its previous rows"
    );

    // -- Exec: the frame pays.
    store.ensure_height_index(40);

    // -- Check: the real geometry landed.
    assert!(
        store.total_visual_rows() > rows_before,
        "the frame measures the new body"
    );
    Ok(())
}

/// WHEN the diff gate re-pays its projection, THE store SHALL accumulate the
/// duration and the call, and THE arm reports their delta.
#[test]
fn test_transcript_store_diff_gate_cost_accumulates_per_projection() -> Result<()> {
    // -- Setup & Fixtures
    let mut store = TranscriptStore::default();
    let (millis_before, calls_before) = store.diff_gate_cost();

    // -- Exec: the gate's own call site records one projection per payment.
    store.record_diff_gate_cost(7);
    store.record_diff_gate_cost(3);
    let (millis_after, calls_after) = store.diff_gate_cost();

    // -- Check
    assert_eq!(
        calls_after.saturating_sub(calls_before),
        2,
        "each projection is one call"
    );
    assert_eq!(
        millis_after.saturating_sub(millis_before),
        10,
        "the durations accumulate"
    );
    Ok(())
}

/// WHEN the store's counters already carry totals before the arm begins, THE
/// breakdown SHALL render the arm's own deltas, not the running totals: the
/// `reset`→`close_store` arithmetic on nonzero baselines.
///
/// The breakdown is driven through the coordinator, which owns the arm timing:
/// the store's totals are its inputs and the coordinator's own `begin`/`close`
/// pair is the production path the warn line takes.
#[test]
fn test_transcript_store_nonzero_baselines_render_the_arm_deltas() -> Result<()> {
    // -- Setup & Fixtures: a store that already paid one re-measure and one gate
    // projection before the arm began.
    let mut coordinator = RuntimeCoordinator::new(80, 24, None);
    coordinator
        .state
        .transcript
        .push_entry(user_entry("entry 0"));
    coordinator.state.transcript.ensure_height_index(40);
    coordinator.state.transcript.mark_stale(0);
    coordinator.state.transcript.ensure_height_index(40);
    coordinator.state.transcript.record_diff_gate_cost(5);
    let stale_baseline = coordinator.state.transcript.remeasure_cost();
    let gate_baseline = coordinator.state.transcript.diff_gate_cost();
    assert!(
        stale_baseline.1 > 0 && gate_baseline.1 > 0,
        "the fixture must carry nonzero baselines: {stale_baseline:?} {gate_baseline:?}"
    );

    // -- Exec: the arm grows both counters by one, then closes.
    coordinator.begin_stream_arm();
    coordinator.state.transcript.mark_stale(0);
    coordinator.state.transcript.ensure_height_index(40);
    coordinator.state.transcript.record_diff_gate_cost(9);
    let line = coordinator.stream_arm_breakdown(4);

    // -- Check: the fields carry this arm's deltas, never the store's totals.
    let stale_field = line
        .split_whitespace()
        .find(|field| field.starts_with("stale-remeasure="))
        .ok_or("the breakdown must name the re-measure delta")?;
    let gate_field = line
        .split_whitespace()
        .find(|field| field.starts_with("diff-gate="))
        .ok_or("the breakdown must name the diff-gate delta")?;
    assert!(
        stale_field.ends_with("/1@0"),
        "one re-measure of entry 0, not the store's total: {line}"
    );
    assert_eq!(
        gate_field, "diff-gate=9ms/1",
        "the gate's own 9 ms projection, not the store's total: {line}"
    );
    Ok(())
}
