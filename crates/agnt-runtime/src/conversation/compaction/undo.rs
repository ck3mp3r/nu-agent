//! The undo of a compaction: the newest summary replaced back by the entries it
//! replaced.
//!
//! rig-memory 0.47 has no undo of its own; what it has is the record the undo
//! needs. A summary's splice took the entries it read out of the conversation,
//! and the log keeps them as they were just before it, keyed by the summary's
//! id ([`rig_memory::summarized_in_log`]). The newest leading summary is where a
//! compaction landed ([`rig_memory::summarized_prefix`]); putting its originals
//! back in its place is the exact inverse of that one splice.

use rig::conversation::Conversation;
use rig_memory::summary_author;

use agnt_kernel::bus::LifecycleEvent;
use agnt_kernel::{CompactionRecord, CompactionSource, NoticeKind, NoticeRecord, SessionRecord};

use crate::bus::{LifecycleTx, Stream};
use crate::session::CachedMemory;

// region:    --- Types

/// Why the undo put nothing back.
#[derive(Debug, thiserror::Error)]
pub(crate) enum UndoError {
    /// The conversation has no compaction left to undo.
    #[error("there is no compaction to undo")]
    Nothing,
    /// The session's log does not hold what it replaced, such as before the
    /// compaction was saved.
    #[error("the session's log does not hold what it replaced")]
    NotInLog,
    /// The session's log could not be read.
    #[error(transparent)]
    Load(#[from] rig_memory::SessionError),
}

// endregion: --- Types

// region:    --- Functions

/// Undo the newest compaction of `conversation_id`, publishing what it did.
///
/// The store is read and written on the blocking pool. A restore publishes the
/// `Uncompacted` record and the spine row; a refusal publishes a warning notice
/// with the reason and the spine's `UncompactFailed` row, and changes nothing.
///
/// # Errors
///
/// The reason the undo did not run, as text, after it has been published.
pub(crate) async fn uncompact(
    conversation_id: &str,
    memory: &CachedMemory,
    source: CompactionSource,
    stream: &Stream,
    lifecycle: &LifecycleTx,
) -> Result<(), String> {
    let result = match memory.rig_store(conversation_id).await {
        Ok(store) => {
            let log = conversation_id.to_owned();
            let undone = crate::session::blocking::on_pool(move || undo(&*store, &log)).await;
            match undone {
                Ok(outcome) => outcome,
                Err(e) => Err(format!("Uncompaction pool error: {e}")),
            }
        }
        Err(e) => Err(format!("Uncompaction store error: {e}")),
    };

    match result {
        Ok(()) => {
            let _ = stream.publish(SessionRecord::Compaction(CompactionRecord::Uncompacted {
                source,
            }));
            let _ = lifecycle.publish(LifecycleEvent::Uncompacted { source });
            Ok(())
        }
        Err(message) => {
            let _ = stream.publish(SessionRecord::Notice(NoticeRecord {
                kind: NoticeKind::Warning,
                text: format!("Nothing undone: {message}"),
            }));
            let _ = lifecycle.publish(LifecycleEvent::UncompactFailed {
                source,
                message: message.clone(),
            });
            Err(message)
        }
    }
}

// endregion: --- Functions

// region:    --- Support

/// Load the conversation, put back what its newest compaction replaced, and
/// save.
///
/// Nothing is written when the undo refused: the refusal is decided before the
/// first splice, so the loaded copy is dropped unsaved.
fn undo(store: &dyn rig_memory::Store, log: &str) -> Result<(), String> {
    let mut conversation =
        rig_memory::load(store, log).map_err(|e| format!("Uncompaction load error: {e}"))?;
    undo_newest(&mut conversation, store, log).map_err(|e| e.to_string())?;
    rig_memory::save(store, log, &mut conversation)
        .map_err(|e| format!("Uncompaction save error: {e}"))?;
    Ok(())
}

/// Put the entries the newest leading summary replaced back in its place.
fn undo_newest(
    conversation: &mut Conversation,
    store: &dyn rig_memory::Store,
    log: &str,
) -> Result<(), UndoError> {
    let Some(newest) = rig_memory::summarized_prefix(conversation).checked_sub(1) else {
        return Err(UndoError::Nothing);
    };
    let Some(summary) = conversation.id(newest) else {
        return Err(UndoError::Nothing);
    };
    let mut replaced = rig_memory::summarized_in_log(store, log)?;
    let originals = replaced.remove(&summary).ok_or(UndoError::NotInLog)?;
    conversation.restore(newest..=newest, originals, summary_author());
    Ok(())
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/conversation/compaction/uncompact.rs"]
mod uncompact_test;

// endregion: --- Tests
