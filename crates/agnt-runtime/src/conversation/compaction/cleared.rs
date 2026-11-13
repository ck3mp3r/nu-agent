//! The between-runs clearing splice: the outputs a near-window clearing dropped
//! are replaced in the store once, after pass one's save and before pass two's
//! load.
//!
//! rig's runner loads the conversation before each run and saves it once the run
//! succeeds, so a splice made during a run is wiped by that run's own save. The
//! only window in which the cleared outputs can land is between the two runs.

use std::sync::Arc;

use rig_memory::summary_author;

use agnt_kernel::bus::LifecycleEvent;
use agnt_kernel::{CompactionRecord, CompactionSource, SessionRecord};

use crate::bus::{LifecycleTx, Stream};
use crate::hook::chain::PendingClearance;

// region:    --- Functions

/// Land stashed `clearance` in `store` under `log`: rewrite the outputs it drops
/// and embed its notes, then save once.
///
/// The clearing is published as finished on the record stream and the lifecycle
/// spine, so a surface sees it even though it ran in the turn executor. Returns
/// whether it landed.
pub(crate) async fn land(
    clearance: PendingClearance,
    store: Arc<dyn rig_memory::Store>,
    log: String,
    stream: &Stream,
    lifecycle: &LifecycleTx,
) -> bool {
    let written = crate::session::blocking::on_pool(move || apply(&*store, &log, clearance)).await;
    let outcome = match written {
        Ok(Ok(())) => Ok(()),
        Ok(Err(message)) => Err(message),
        Err(e) => Err(format!("Compaction splice error: {e}")),
    };
    match outcome {
        Ok(()) => {
            let _ = stream.publish(SessionRecord::Compaction(CompactionRecord::Completed {
                source: CompactionSource::Auto,
                summary: String::new(),
            }));
            let _ = lifecycle.publish(LifecycleEvent::CompactionCompleted {
                source: CompactionSource::Auto,
                summary: String::new(),
            });
            true
        }
        Err(message) => {
            let _ = stream.publish(SessionRecord::Compaction(CompactionRecord::Failed {
                source: CompactionSource::Auto,
                message: message.clone(),
            }));
            let _ = lifecycle.publish(LifecycleEvent::CompactionFailed {
                source: CompactionSource::Auto,
                message,
            });
            false
        }
    }
}

// endregion: --- Functions

// region:    --- Support

/// Load the conversation, apply the clearing's plan to it, and save.
///
/// The plan names entries by id, so it applies to the conversation the run just
/// grew; an entry it no longer finds is left alone, and applying it twice
/// changes nothing the second time.
fn apply(
    store: &dyn rig_memory::Store,
    log: &str,
    clearance: PendingClearance,
) -> Result<(), String> {
    let mut conversation =
        rig_memory::load(store, log).map_err(|e| format!("Compaction load error: {e}"))?;
    clearance
        .plan()
        .apply(&mut conversation, clearance.notes(), summary_author());
    rig_memory::save(store, log, &mut conversation)
        .map_err(|e| format!("Compaction save error: {e}"))
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/conversation/compaction/clearing.rs"]
mod clearing_test;

// endregion: --- Tests
