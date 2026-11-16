//! The compactor is the slot's second consumer, so it yields on a build.
//!
//! A model switch leaves the shared slot stale and builds nothing. The sweep does
//! not wait for that build: it publishes the request it is — asked for, not
//! started — and returns, so a slow or wedged provider client holds nobody. The
//! turn executor owns the build, and the next attempt after it lands sweeps.

use agnt_kernel::{CompactionRecord, CompactionSource, SessionRecord};

use super::harness::*;
use super::*;
use crate::session::management::SessionPersistence;

/// The compaction rows `rx` carries, in publication order.
fn compaction_rows(
    rx: &mut agnt_kernel::bus::Subscription<SessionRecord>,
) -> Vec<CompactionRecord> {
    let mut rows = Vec::new();
    while let Ok(record) = rx.try_recv() {
        if let SessionRecord::Compaction(row) = record {
            rows.push(row);
        }
    }
    rows
}

/// WHEN a switch left the slot stale, THEN `run_compaction` SHALL publish
/// `Requested`, start no sweep, and leave the slot stale for the next use.
#[tokio::test]
async fn a_stale_slot_yields_the_manual_sweep() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    let mut rows = fixture.runtime.bus.stream().subscribe();
    fixture.runtime.dyn_model.mark_pending();

    // -- Exec
    fixture
        .runtime
        .run_compaction(CompactionSource::Manual)
        .await
        .map_err(|e| format!("a yielded sweep is not a failure: {e}"))?;

    // -- Check: the request is the only row, and the debt is still owed.
    let published = compaction_rows(&mut rows);
    assert_eq!(
        published,
        vec![CompactionRecord::Requested {
            source: CompactionSource::Manual
        }],
        "a sweep that cannot read the model reports the request only"
    );
    assert!(
        !published
            .iter()
            .any(|row| matches!(row, CompactionRecord::Started { .. })),
        "no sweep started"
    );
    assert!(
        fixture.runtime.dyn_model.is_pending(),
        "the slot stays stale: the build belongs to the turn executor"
    );
    Ok(())
}

/// WHILE a consumer owns the build, THEN `run_compaction` SHALL yield the same
/// way; WHEN that build lands, THEN the next attempt SHALL take the sweep.
#[tokio::test]
async fn the_sweep_takes_the_slot_once_the_build_lands() -> Result<()> {
    // -- Setup & Fixtures
    let mut fixture = make_runtime(text_model("unused"))?;
    let mut rows = fixture.runtime.bus.stream().subscribe();
    fixture.runtime.dyn_model.mark_pending();

    // -- Exec: the attempt mid-build yields.
    fixture.runtime.dyn_model.mark_building();
    fixture
        .runtime
        .run_compaction(CompactionSource::Manual)
        .await
        .map_err(|e| format!("the yielded attempt is not a failure: {e}"))?;
    assert_eq!(
        compaction_rows(&mut rows),
        vec![CompactionRecord::Requested {
            source: CompactionSource::Manual
        }],
        "a build in flight yields the sweep"
    );

    // -- Exec: the build lands, then the next attempt sweeps.
    fixture
        .runtime
        .dyn_model
        .install(rig::test_utils::MockCompletionModel::default().erase());
    fixture
        .runtime
        .run_compaction(CompactionSource::Manual)
        .await
        .map_err(|e| format!("the sweep should run: {e}"))?;

    // -- Check: no second request, and the sweep reached its own ladder.
    let published = compaction_rows(&mut rows);
    assert!(
        !published
            .iter()
            .any(|row| matches!(row, CompactionRecord::Requested { .. })),
        "an attempt that reads the model asks for nothing; got {published:?}"
    );
    assert!(
        published.iter().any(|row| matches!(
            row,
            CompactionRecord::Completed { .. } | CompactionRecord::Started { .. }
        )),
        "the sweep reached the compaction ladder; got {published:?}"
    );
    Ok(())
}
