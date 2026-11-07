//! Hook-driven compaction: the threshold decision, the store read, and the
//! splice that runs when the orchestrator dispatches a compaction.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use rig::message::Message;
use rig_memory::CompactReason;

use agnt_kernel::CompactionSource;
use agnt_storage::StorageError;

use crate::bus::CompactionTx;
use crate::conversation::compaction::CompactionConfig;
use crate::conversation::turn::token_estimate::estimate_token_count;
use crate::protocol::events::{SystemEvent, compaction_source_text};

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
    memory: &crate::session::CachedMemory,
    compaction: &CompactionConfig,
    last_total_tokens: &Arc<Mutex<Option<u64>>>,
    store_latch: &crate::hook::StoreLatch,
    compaction_tx: &CompactionTx,
) {
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

    let Some(conversation) =
        current_conversation(memory, conversation_id, store_latch, compaction_tx).await
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

    // Compaction is needed. Fire a `SystemEvent::CompactionRequested` on the
    // channel so the orchestrator schedules the compaction on the worker; the
    // splice it makes is what the next run loads.
    let _ = compaction_tx
        .send(SystemEvent::CompactionRequested {
            source: CompactionSource::Auto,
        })
        .await;
}

/// The conversation the session's rig-memory store holds right now.
///
/// Store errors are surfaced as a `SystemEvent::CompactionFailed` on the
/// compaction channel and yield `None` so the turn proceeds without a
/// compaction decision.
async fn current_conversation(
    memory: &crate::session::CachedMemory,
    conversation_id: &str,
    store_latch: &crate::hook::StoreLatch,
    compaction_tx: &CompactionTx,
) -> Option<Vec<Message>> {
    let store = match memory.rig_store(conversation_id).await {
        Ok(store) => store,
        Err(e) => {
            emit_store_dead(store_latch, &e, "auto", compaction_tx).await;
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
            let _ = compaction_tx
                .send(SystemEvent::CompactionFailed {
                    source: "auto".to_string(),
                    message: format!("Failed to load the session: {e}"),
                })
                .await;
            None
        }
        Err(e) => {
            let _ = compaction_tx
                .send(SystemEvent::CompactionFailed {
                    source: "auto".to_string(),
                    message: format!("Failed to load the session: {e}"),
                })
                .await;
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
    store_latch: &crate::hook::StoreLatch,
    error: &crate::session::MemoryError,
    source: &str,
    compaction_tx: &CompactionTx,
) {
    let message = store_refusal_text(error);
    let first = {
        let mut latch = store_latch
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
        let _ = compaction_tx
            .send(SystemEvent::CompactionFailed {
                source: source.to_string(),
                message,
            })
            .await;
    }
}

/// The clean refusal line a store failure reports.
///
/// The storage refusal itself is the line when the error chain carries one;
/// the wrappers above it (`Memory backend error: …`, the sqlx wording) are
/// never part of what the user reads.
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
    memory: &crate::session::CachedMemory,
    compaction: &CompactionConfig,
    source: CompactionSource,
    last_total_tokens: &Arc<Mutex<Option<u64>>>,
    store_latch: &crate::hook::StoreLatch,
    compaction_tx: &CompactionTx,
) -> Option<Vec<Message>> {
    let source_text = compaction_source_text(source);
    let store = match memory.rig_store(conversation_id).await {
        Ok(store) => store,
        Err(e) => {
            emit_store_dead(store_latch, &e, source_text, compaction_tx).await;
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
        .compact(store, conversation_id, &reason, source_text)
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
            let _ = compaction_tx
                .send(SystemEvent::CompactionCompleted {
                    source: source_text.to_string(),
                    summary_preview: String::new(),
                    summary_body: String::new(),
                })
                .await;
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
