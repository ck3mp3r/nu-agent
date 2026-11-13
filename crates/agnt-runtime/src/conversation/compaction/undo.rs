//! The undo of a compaction: rig-memory's restore, minus pass one's internal
//! reply.
//!
//! `rig_memory::undo_summary` puts back the entries the newest compaction's
//! summary replaced. An in-place compaction's reply is one of the model's own
//! answers to its internal summary call, so it must not come back into the
//! transcript: S3's splice removes that entry after the summary lands, and the
//! same identification removes it again when the restore put it back.

use rig::conversation::{Conversation, MessageId};
use rig_memory::summary_author;

use agnt_kernel::bus::LifecycleEvent;
use agnt_kernel::{CompactionRecord, CompactionSource, NoticeKind, NoticeRecord, SessionRecord};

use crate::bus::{LifecycleTx, Stream};
use crate::session::CachedMemory;

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

/// Load the conversation, undo its newest compaction, drop pass one's reply
/// from what the undo put back, and save.
///
/// Nothing is written when the undo refused: `undo_summary` changes nothing on
/// its error paths, so the loaded copy is dropped unsaved.
fn undo(store: &dyn rig_memory::Store, log: &str) -> Result<(), String> {
    let mut conversation =
        rig_memory::load(store, log).map_err(|e| format!("Uncompaction load error: {e}"))?;
    // The summary the newest compaction landed is pass one's reply text: the
    // reply entry is identified by the same text S3's cleanup uses.
    let reply = newest_summary(&conversation);
    let undone = rig_memory::undo_summary(&mut conversation, store, log, summary_author())
        .map_err(|e| e.to_string())?;
    if let Some(reply) = reply {
        remove_restored_reply(&mut conversation, &undone.restored, &reply);
    }
    rig_memory::save(store, log, &mut conversation)
        .map_err(|e| format!("Uncompaction save error: {e}"))?;
    Ok(())
}

/// The text of the newest compaction's summary, `None` when the conversation
/// holds no compaction.
fn newest_summary(conversation: &Conversation) -> Option<String> {
    rig_memory::Compacted::of(conversation)
        .last()
        .map(|(_, compacted)| compacted.state.summary)
        .filter(|summary| !summary.is_empty())
}

/// Remove pass one's reply from the entries the undo restored.
///
/// Only the restored entries are considered: an entry the compaction kept was
/// never pass one's reply to take. The reply is found the way S3's cleanup
/// finds it — an entry carrying no author whose own text is the reply's — and
/// its paired tool records go with it, so no result answers a call that is
/// gone.
fn remove_restored_reply(conversation: &mut Conversation, restored: &[MessageId], reply: &str) {
    let found = conversation
        .entries()
        .rev()
        .find(|entry| {
            restored.contains(&entry.id)
                && entry.author.is_none()
                && super::supersede::is_reply_entry(entry.message, reply)
        })
        .map(|entry| (entry.index, super::supersede::reply_calls(entry.message)));
    let Some((at, calls)) = found else {
        return;
    };
    conversation.splice(at..at + 1, Vec::new(), summary_author());
    super::supersede::remove_paired_tool_records(conversation, at, &calls);
}

// endregion: --- Support

// region:    --- Tests

#[cfg(test)]
#[path = "../../../test/conversation/compaction/uncompact.rs"]
mod uncompact_test;

// endregion: --- Tests
