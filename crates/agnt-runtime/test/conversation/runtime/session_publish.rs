//! The producer half of the session-boundary contract: what `new_session`
//! publishes on the record stream and on the lifecycle spine.
//!
//! A surface derives the transcript clear from the `Started` record, so a
//! publish that silently stops happening — or that flips its `hydrated` flag —
//! breaks `/new` without breaking any consumer test. Nothing else pinned these
//! two publishes.

use super::harness::*;
use super::*;
use crate::protocol::events::{LifecycleEvent, SessionLifecycle, SessionRecord};
use crate::session::management::SessionState;

/// WHEN `new_session` mints a session, THE runtime SHALL publish exactly one
/// `Started { hydrated: false }` record on the stream and exactly one
/// `SessionStarted { hydrated: false }` on the lifecycle spine, both naming the
/// session it minted.
#[tokio::test]
async fn new_session_publishes_the_started_record_and_the_spine_row() -> Result<()> {
    // -- Setup & Fixtures: subscribe before the mint — both concerns are
    // zero-retention, so a later subscriber would see nothing.
    let mut fixture = make_runtime(text_model("unused"))?;
    let mut stream = fixture.runtime.bus.stream().subscribe();
    let mut lifecycle = fixture.runtime.bus.lifecycle().subscribe();

    // -- Exec
    fixture.runtime.new_session();

    // -- Check
    let minted = fixture
        .runtime
        .final_session_id
        .clone()
        .ok_or("new_session must set a session id")?;
    let record = stream
        .try_recv()
        .map_err(|e| format!("the mint must publish a stream record: {e}"))?;
    match record {
        SessionRecord::Session(SessionLifecycle::Started {
            session_id,
            hydrated,
        }) => {
            assert_eq!(
                session_id.as_str(),
                minted,
                "the record must name the session the runtime minted"
            );
            assert!(!hydrated, "a minted session carries no hydrated history");
        }
        other => return Err(format!("expected a started session record; got {other:?}").into()),
    }
    assert!(
        stream.try_recv().is_err(),
        "the mint publishes exactly one stream record"
    );

    let event = lifecycle
        .try_recv()
        .map_err(|e| format!("the mint must publish a spine row: {e}"))?;
    match event {
        LifecycleEvent::SessionStarted {
            session_id,
            hydrated,
        } => {
            assert_eq!(
                session_id, minted,
                "the spine row must name the session the runtime minted"
            );
            assert!(!hydrated, "a minted session carries no hydrated history");
        }
        other => return Err(format!("expected a session-started spine row; got {other:?}").into()),
    }
    assert!(
        lifecycle.try_recv().is_err(),
        "the mint publishes exactly one spine row"
    );
    Ok(())
}
