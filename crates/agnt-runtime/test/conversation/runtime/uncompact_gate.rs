//! The undo's dispatch gate: `/uncompact` answers from the same read as its row.
//!
//! A session with no summary in it has nothing to put back. The guard publishes
//! the pair it read and the reason, and dispatches no undo — the mirror of the
//! sweep's refusal in `run_compaction`.

use agnt_kernel::bus::{CommandFacts, CompactionAvailability, FactRecord, Subscription};
use agnt_kernel::{CompactionRecord, CompactionSource, NoticeKind, SessionRecord};

use super::harness::*;
use super::*;
use crate::conversation::compaction::availability::{NOTHING_TO_SUMMARIZE, NOTHING_TO_UNDO};
use crate::session::management::SessionPersistence;

/// The records `rx` carries, in publication order.
fn records(rx: &mut Subscription<SessionRecord>) -> Vec<SessionRecord> {
    let mut out = Vec::new();
    while let Ok(record) = rx.try_recv() {
        out.push(record);
    }
    out
}

/// WHEN `/uncompact` runs on a conversation that holds no compaction, THEN THE
/// runtime SHALL publish the pair it read and the refusal notice, and SHALL NOT
/// run the undo: no `Uncompacted` row, no write.
#[tokio::test]
async fn test_conversation_runtime_uncompact_refuses_with_the_fact_reason() -> Result<()> {
    // -- Setup & Fixtures: a runtime attached to an empty in-memory conversation.
    let mut fixture = make_runtime(text_model("unused"))?;
    let mut rows = fixture.runtime.bus.stream().subscribe();
    let mut facts = fixture.runtime.bus.facts().subscribe();

    // -- Exec
    fixture
        .runtime
        .uncompact(CompactionSource::Manual)
        .await
        .map_err(|e| format!("a refusal is the notice, not a failure: {e}"))?;

    // -- Check: the pair says the undo has nothing to put back, and the sweep has
    // nothing to cut either — the session holds no conversation yet.
    let published = facts.try_recv()?;
    assert_eq!(
        published,
        FactRecord::CommandAvailability(CommandFacts {
            compact: CompactionAvailability::Unavailable(NOTHING_TO_SUMMARIZE.to_string()),
            uncompact: CompactionAvailability::Unavailable(NOTHING_TO_UNDO.to_string()),
        }),
        "the refusal publishes the pair the guard read"
    );

    // -- Check: the reason is the notice the undo is owed, and no undo ran.
    let records = records(&mut rows);
    assert_eq!(records.len(), 1, "the refusal answers once: {records:?}");
    assert!(
        matches!(
            &records[0],
            SessionRecord::Notice(notice)
                if notice.kind == NoticeKind::Warning && notice.text == NOTHING_TO_UNDO
        ),
        "the refusal publishes the fact's reason as a warning: {records:?}"
    );
    assert!(
        !records.iter().any(|record| matches!(
            record,
            SessionRecord::Compaction(CompactionRecord::Uncompacted { .. })
        )),
        "no undo ran: {records:?}"
    );
    Ok(())
}
