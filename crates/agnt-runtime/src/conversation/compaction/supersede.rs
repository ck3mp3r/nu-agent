//! The between-runs splice: the side-call's summary becomes the conversation's
//! summary, written after pass one's save and before pass two's load.
//!
//! rig's runner loads the conversation before each run and saves it once the run
//! succeeds, so a splice made during a run is wiped by that run's own save. The
//! only window in which a summary can land is between the two runs: the store
//! then holds pass one's exchange, and pass two loads what this module wrote.

use std::sync::Arc;

use agnt_kernel::bus::LifecycleEvent;
use agnt_kernel::{CompactionRecord, SessionRecord};

use crate::bus::{LifecycleTx, Stream};
use crate::hook::chain::Supersede;

// region:    --- Types

/// What the splice did with the summary.
#[derive(Debug)]
pub(crate) enum Landed {
    /// The summary is in the conversation and the store holds it.
    Summary(String),
    /// The summary could not be landed; nothing was inserted.
    Rejected(String),
}

// endregion: --- Types

// region:    --- Functions

/// Land the side-call's summary in `store` under `log`: splice the summary in and
/// save.
///
/// Every outcome is published on the record stream and the lifecycle spine, so a
/// surface sees the compaction finish even though it ran in the turn executor:
/// `Completed` when the summary landed, `Failed` when it could not.
///
/// A cancelled turn calls nothing here: it gets no second run, so no summary is
/// spliced for it.
pub(crate) async fn land(
    supersede: Supersede,
    store: Arc<dyn rig_memory::Store>,
    log: String,
    stream: &Stream,
    lifecycle: &LifecycleTx,
) -> Landed {
    let source = supersede.source();
    let spliced = crate::session::blocking::on_pool(move || splice(&*store, &log, supersede)).await;
    let landed = match spliced {
        Ok(Ok(landed)) => landed,
        Ok(Err(message)) => Landed::Rejected(message),
        Err(e) => Landed::Rejected(format!("Compaction splice error: {e}")),
    };
    match &landed {
        Landed::Summary(summary) => {
            let _ = stream.publish(SessionRecord::Compaction(CompactionRecord::Completed {
                source: Some(source),
                summary: summary.clone(),
            }));
            let _ = lifecycle.publish(LifecycleEvent::CompactionCompleted {
                source,
                summary: summary.clone(),
            });
        }
        Landed::Rejected(message) => {
            let _ = stream.publish(SessionRecord::Compaction(CompactionRecord::Failed {
                source,
                message: message.clone(),
            }));
            let _ = lifecycle.publish(LifecycleEvent::CompactionFailed {
                source,
                message: message.clone(),
            });
        }
    }
    landed
}

// endregion: --- Functions

// region:    --- Support

/// Load the conversation, put the summary in place of the messages it replaced,
/// and save.
///
/// The side-call asked for the summary on its own, so no entry in the stored
/// conversation holds it: the write here is the only place it enters the
/// history.
///
/// `apply_summary` recognizes the messages it read by their own allocations, so
/// the replacement set is re-read here, from the conversation this write holds:
/// `prepare_compaction_at` cuts the same prefix (the side-call summarized the
/// first `replaces.len()` messages) and refuses the splice when the conversation
/// no longer keeps from there. Nothing else writes the store, so the prefix is
/// the one the side-call read.
fn splice(
    store: &dyn rig_memory::Store,
    log: &str,
    supersede: Supersede,
) -> Result<Landed, String> {
    let mut conversation =
        rig_memory::load(store, log).map_err(|e| format!("Compaction load error: {e}"))?;
    let cut = supersede.replaces().len();
    let preparation = rig_memory::prepare_compaction_at(&conversation, cut)
        .ok_or_else(|| "Compaction summary could not be spliced in".to_string())?;
    let text = supersede.reply().to_owned();
    let mut state = supersede.state().clone();
    state.summary = text.clone();
    rig_memory::apply_summary(&mut conversation, &preparation.replaces, &state)
        .ok_or_else(|| "Compaction summary could not be spliced in".to_string())?;
    rig_memory::save(store, log, &mut conversation)
        .map_err(|e| format!("Compaction save error: {e}"))?;
    Ok(Landed::Summary(text))
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/conversation/compaction/supersede.rs"]
mod supersede_test;

// endregion: --- Tests
