//! Hook-driven compaction: the threshold decision, the store read, and the
//! splice that runs when the orchestrator dispatches a compaction.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rig::message::Message;
use rig_memory::CompactReason;

use agnt_kernel::bus::{ControlCommand, LifecycleEvent};
use agnt_kernel::{CompactionRecord, CompactionSource, SessionRecord};
use agnt_storage::StorageError;

use crate::bus::{ControlTx, LifecycleTx, Stream};
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::turn::token_estimate::estimate_token_count;

// region:    --- Types

/// The inputs every step of the compaction path shares: the session's store, its
/// compaction policy, the last real token count, and the store latch.
///
/// They travel together because every step needs the same four, and passing them
/// one by one leaves no room for the channels each step also publishes on.
pub struct CompactionPath<'a> {
    /// The session's rig-memory store.
    pub memory: &'a crate::session::CachedMemory,
    /// The compaction policy the session decided on.
    pub compaction: &'a CompactionConfig,
    /// The real token count of the last completion, when one was counted.
    pub last_total_tokens: &'a Arc<Mutex<Option<u64>>>,
    /// The latch an unreadable store sets.
    pub store_latch: &'a crate::hook::StoreLatch,
}

// endregion: --- Types

// region:    --- Implementations

impl<'a> CompactionPath<'a> {
    pub fn new(
        memory: &'a crate::session::CachedMemory,
        compaction: &'a CompactionConfig,
        last_total_tokens: &'a Arc<Mutex<Option<u64>>>,
        store_latch: &'a crate::hook::StoreLatch,
    ) -> Self {
        Self {
            memory,
            compaction,
            last_total_tokens,
            store_latch,
        }
    }
}

// endregion: --- Implementations

/// Decide whether this turn should schedule a compaction.
///
/// The threshold is evaluated against the CURRENT conversation of the session's
/// rig-memory store — the store a run loads at its start — so a turn saved after
/// the cache was populated is counted. `rig_memory::load` re-reads and folds the
/// log on every call, so it is always current; the first-read cache is not a
/// compaction input.
///
/// The hook never patches the per-turn history: after a compaction the splice is
/// saved and rig loads it at the next run.
pub(super) async fn decide_compaction(
    prompt: &Message,
    conversation_id: &str,
    path: &CompactionPath<'_>,
    control: &ControlTx,
    stream: &Stream,
    lifecycle: &LifecycleTx,
) {
    let CompactionPath {
        compaction,
        last_total_tokens,
        store_latch,
        ..
    } = path;
    // A store that already refused to open makes no further attempt: the
    // conversation cannot be read, so no decision can be made from it, and the
    // one clean line was emitted when the latch was set.
    if store_latch
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .is_some()
    {
        return;
    }
    let Some(threshold_tokens) = compaction.threshold_tokens else {
        return;
    };
    // A session whose provider name resolved to no rig provider id has no model
    // spec, so its compactor cannot run: scheduling one would only emit a
    // misleading `Requested`.
    if compaction.spec().is_none() {
        return;
    }

    let Some(conversation) = current_conversation(conversation_id, path, stream, lifecycle).await
    else {
        return;
    };
    if conversation.is_empty() {
        return;
    }

    // Threshold check: use the real token count from the last completion when
    // available (it reflects the actual context the model processed last turn,
    // which already includes the prior prompt), plus a chars/4 estimate for the
    // current prompt. When no real count exists (first turn, or after a
    // compaction reset) fall back to the chars/4 estimate for the conversation.
    let base_tokens = lock_last_total_tokens_arc(last_total_tokens)
        .and_then(|n| u64::try_into(n).ok())
        .unwrap_or_else(|| estimate_token_count(&conversation));
    let prompt_estimate = estimate_token_count(std::slice::from_ref(prompt));
    if base_tokens + prompt_estimate < threshold_tokens {
        return;
    }

    // Compaction is needed. Publish the request on the control channel so the
    // orchestrator schedules the compaction on the worker; the splice it makes
    // is what the next run loads.
    let _ = control.publish(ControlCommand::CompactionRequested {
        source: CompactionSource::Auto,
    });
}

/// The conversation the session's rig-memory store holds right now.
///
/// Store errors are surfaced as a `CompactionRecord::Failed` on the stream and
/// a `LifecycleEvent::CompactionFailed` on the spine, and yield `None` so the
/// turn proceeds without a compaction decision.
async fn current_conversation(
    conversation_id: &str,
    path: &CompactionPath<'_>,
    stream: &Stream,
    lifecycle: &LifecycleTx,
) -> Option<Vec<Message>> {
    let store = match path.memory.rig_store(conversation_id).await {
        Ok(store) => store,
        Err(e) => {
            emit_store_dead(path, &e, CompactionSource::Auto, stream, lifecycle).await;
            return None;
        }
    };
    let log = conversation_id.to_owned();
    let loaded = crate::session::blocking::on_pool(move || rig_memory::load(&*store, &log)).await;
    match loaded {
        Ok(Ok(conversation)) => Some(
            conversation
                .messages()
                .iter()
                .map(|message| (**message).clone())
                .collect(),
        ),
        Ok(Err(e)) => {
            emit_compaction_failed(
                stream,
                lifecycle,
                CompactionSource::Auto,
                &format!("Failed to load the session: {e}"),
            );
            None
        }
        Err(e) => {
            emit_compaction_failed(
                stream,
                lifecycle,
                CompactionSource::Auto,
                &format!("Failed to load the session: {e}"),
            );
            None
        }
    }
}

/// Latch a store that cannot be opened and emit its one clean refusal line.
///
/// The latch makes the compaction path skip every further attempt, so an
/// unreadable store costs one line rather than one per completion call. A
/// second refusal finds the latch set and emits nothing.
async fn emit_store_dead(
    path: &CompactionPath<'_>,
    error: &crate::session::MemoryError,
    source: CompactionSource,
    stream: &Stream,
    lifecycle: &LifecycleTx,
) {
    let message = store_refusal_text(error);
    let first = {
        let mut latch = path
            .store_latch
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if latch.is_some() {
            false
        } else {
            *latch = Some(message.clone());
            true
        }
    };
    if first {
        emit_compaction_failed(stream, lifecycle, source, &message);
    }
}

/// Publish one compaction failure: the record every folding consumer reads and
/// the spine row the ordered consumers read.
fn emit_compaction_failed(
    stream: &Stream,
    lifecycle: &LifecycleTx,
    source: CompactionSource,
    message: &str,
) {
    let _ = stream.publish(SessionRecord::Compaction(CompactionRecord::Failed {
        source,
        message: message.to_string(),
    }));
    let _ = lifecycle.publish(LifecycleEvent::CompactionFailed {
        source,
        message: message.to_string(),
    });
}

/// The clean refusal line a store failure reports.
///
/// The storage refusal itself is the line when the error chain carries one;
/// the wrapper above it (`Memory backend error: …`) is never part of what the
/// user reads.
fn store_refusal_text(error: &crate::session::MemoryError) -> String {
    let mut current = std::error::Error::source(error);
    while let Some(source) = current {
        if let Some(storage) = source.downcast_ref::<StorageError>() {
            return storage.to_string();
        }
        current = source.source();
    }
    std::error::Error::source(error)
        .map(ToString::to_string)
        .unwrap_or_else(|| error.to_string())
}

/// Run a compaction for `conversation_id`, returning the post-splice history
/// when compaction ran, or `None` when there was nothing to compact.
///
/// Called by the router when the orchestrator dispatches a `RunCompaction`
/// worker command. The events after the request, the hook envelopes, and the
/// store carry the source as text, so `compaction_source_text` derives it here
/// once. `compact()` emits all `Started`/`SummaryChunk`/`Completed`/`Failed`
/// events; this function emits a `Completed` with an empty summary when there
/// was nothing to summarize, so the TUI still gets feedback that the `/compact`
/// command ran.
///
/// The conversation is read from the rig-memory store of the session, which is
/// what a run loads at its start: the splice `compact()` makes IS the context
/// from then on.
pub async fn run_compaction(
    conversation_id: &str,
    path: &CompactionPath<'_>,
    source: CompactionSource,
    stream: &Stream,
    lifecycle: &LifecycleTx,
) -> Option<Vec<Message>> {
    let CompactionPath {
        memory,
        compaction,
        last_total_tokens,
        store_latch,
    } = path;
    let store = match memory.rig_store(conversation_id).await {
        Ok(store) => store,
        Err(e) => {
            emit_store_dead(path, &e, source, stream, lifecycle).await;
            return None;
        }
    };
    // The store opened: whatever refused before is gone, so the latch clears
    // and the automatic decision resumes on the next completion.
    *store_latch
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = None;

    let reason = compact_reason(source);
    match compaction
        .compactor
        .compact(store, conversation_id, &reason, source)
        .await
    {
        Ok(Some(outcome)) => {
            // Compaction succeeded: the real token count from the previous turn no
            // longer reflects the (now summarized) context. Reset it so the next
            // turn falls back to the estimate (small — just the summary), then
            // picks up the real count after its first completion.
            *lock_last_total_tokens_arc(last_total_tokens) = None;
            Some(outcome.messages)
        }
        Ok(None) => {
            let _ = stream.publish(SessionRecord::Compaction(CompactionRecord::Completed {
                source,
                summary: String::new(),
            }));
            let _ = lifecycle.publish(LifecycleEvent::CompactionCompleted {
                source,
                summary: String::new(),
            });
            None
        }
        // `compact()` already emitted `Failed`; fall back to the full history so
        // the turn is not lost.
        Err(_) => None,
    }
}

/// Why a compaction is running, from the command that asked for it.
///
/// `Manual` is the user asking through `/compact`; `Auto` is the threshold
/// path.
fn compact_reason(source: CompactionSource) -> CompactReason {
    match source {
        CompactionSource::Manual => CompactReason::Asked {
            focus: String::new(),
        },
        CompactionSource::Auto => CompactReason::Threshold,
    }
}

// region:    --- Support

/// The shared token count a compaction path carries by reference, recovered if
/// a writer panicked mid-update.
fn lock_last_total_tokens_arc(
    last_total_tokens: &Arc<Mutex<Option<u64>>>,
) -> MutexGuard<'_, Option<u64>> {
    last_total_tokens
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

// endregion: --- Support
