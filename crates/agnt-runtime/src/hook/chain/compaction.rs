//! Hook-driven compaction: the threshold decision and the self-contained side-call
//! it schedules, and the splice that runs when the orchestrator dispatches a manual
//! compaction.

// region:    --- Modules

mod side_call;

// endregion: --- Modules

use rig::DynModel;
use rig::agent::CompletionCallAction;
use rig::catalog::ModelSpec;
use rig::conversation::Conversation;
use rig::message::Message;
use rig::operation::Completion;
use rig_memory::request_tokens;

use agnt_kernel::bus::LifecycleEvent;
use agnt_kernel::{CompactionRecord, CompactionSource, SessionRecord};
use agnt_storage::StorageError;

use crate::bus::{LifecycleTx, Stream};
use crate::conversation::compaction::compactor::NuCompactor;
use crate::hook::anchor_meter::AnchorMeter;

use super::PendingSummary;

// region:    --- Types

/// The inputs every step of the compaction path shares: the session's store, its
/// compaction policy, the meter the trigger reads and writes, the store latch,
/// and the wire this turn's calls go to.
///
/// They travel together because every step needs the same five, and passing them
/// one by one leaves no room for the channels each step also publishes on.
pub struct CompactionPath<'a> {
    /// The session's rig-memory store.
    pub memory: &'a crate::session::CachedMemory,
    /// The compactor the session decided on, carrying rig's compaction settings.
    pub compaction: &'a NuCompactor,
    /// The trigger's meter: the usage the last completion reported and the entry
    /// it counts through.
    pub meter: &'a AnchorMeter,
    /// The latch an unreadable store sets.
    pub store_latch: &'a crate::hook::StoreLatch,
    /// The model this turn sends to: every side-call runs on it.
    pub model: &'a DynModel<Completion>,
}

// endregion: --- Types

// region:    --- Implementations

impl<'a> CompactionPath<'a> {
    pub fn new(
        memory: &'a crate::session::CachedMemory,
        compaction: &'a NuCompactor,
        meter: &'a AnchorMeter,
        store_latch: &'a crate::hook::StoreLatch,
        model: &'a DynModel<Completion>,
    ) -> Self {
        Self {
            memory,
            compaction,
            meter,
            store_latch,
            model,
        }
    }
}

// endregion: --- Implementations

/// Decide what this turn's completion call sends, and whether it schedules a
/// summary before the turn's second pass.
///
/// The trigger is rig's: the tokens the next request would send, counted by
/// `request_tokens`, against `should_compact` — the model's input limit less the
/// settings' `reserve_tokens`. The request is the CURRENT conversation of the
/// session's rig-memory store — the store a run loads at its start — plus this
/// turn's prompt, so a turn saved after the cache was populated is counted.
/// `rig_memory::load` re-reads and folds the log on every call, so it is always
/// current; the first-read cache is not a compaction input.
///
/// The entry the request counts through is recorded on the meter, so the
/// completion that settles writes the usage the next decision anchors with. An
/// unknown input limit never reads as over the threshold: `should_compact` needs
/// a window, so the turn continues and no request is scheduled.
///
/// Over the threshold — or after a request the provider refused as too long — the
/// summary is asked for on its own and spliced in between the runs: the turn's
/// own request goes unpatched.
pub(super) async fn decide_compaction(
    prompt: &Message,
    conversation_id: &str,
    spec: Option<&ModelSpec>,
    pending: &PendingSummary,
    path: &CompactionPath<'_>,
    stream: &Stream,
    lifecycle: &LifecycleTx,
) -> CompletionCallAction {
    let CompactionPath {
        compaction,
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
        return CompletionCallAction::continue_run();
    }
    // A model whose input limit is unknown has no threshold to cross, and a
    // session whose provider name resolved to no rig provider id has no spec at
    // all.
    let Some(spec) = spec else {
        return CompletionCallAction::continue_run();
    };

    let Some(mut conversation) =
        current_conversation(conversation_id, path, stream, lifecycle).await
    else {
        return CompletionCallAction::continue_run();
    };
    if conversation.messages().is_empty() {
        return CompletionCallAction::continue_run();
    }
    // The request about to be sent is the store's conversation plus this turn's
    // prompt, so the prompt joins the counted history and its entry is the one
    // the reply's usage counts through.
    conversation.extend([prompt.clone()]);
    let Some(through) = conversation
        .messages()
        .len()
        .checked_sub(1)
        .and_then(|at| conversation.id(at))
    else {
        return CompletionCallAction::continue_run();
    };

    // The anchor is the usage the provider reported for the last reply, plus an
    // estimate of the messages sent after the entry it counted through. The hook
    // holds no system prompt and no tool definitions here, so the whole estimate
    // counts the messages alone.
    let anchor = path.meter.anchor().and_then(|anchor| {
        let tokens = anchor.usage.input_tokens?;
        Some((tokens, conversation.index_of(anchor.through)?))
    });
    let tokens = request_tokens((None, None), conversation.messages(), anchor);
    path.meter.record_request(through);

    // One attempt per turn: the pass that already stashed a summary re-runs, so
    // the summary it asked for stays the turn's only one and no second Started row
    // is published for it.
    if pending.attempted() {
        return CompletionCallAction::continue_run();
    }

    let settings = compaction.settings();
    if !settings.enabled {
        return CompletionCallAction::continue_run();
    }
    // A request the provider refused as too long compacts even when the estimate
    // says it fits: the previous summary may have gone in after its prompt.
    let overflowed = compaction.take_overflowed();
    let Some(window) = spec.input_limit().map(u64::from) else {
        return CompletionCallAction::continue_run();
    };
    if !overflowed && !settings.should_compact(tokens, window) {
        return CompletionCallAction::continue_run();
    }

    // Over the threshold: the summary is asked for on its own, prepared to fit,
    // asked in one self-contained call, and spliced in between the runs by the
    // executor.
    side_call::plan_alone(
        &conversation,
        spec,
        compaction,
        path.model,
        CompactionSource::Auto,
        overflowed,
        pending,
    )
    .await;
    CompletionCallAction::continue_run()
}

/// The conversation the session's rig-memory store holds right now.
///
/// Store errors are surfaced as a `CompactionRecord::Failed` on the stream and
/// a `LifecycleEvent::CompactionFailed` on the spine, and yield `None` so the
/// turn proceeds without a compaction decision. The conversation is returned
/// whole, entries and their ids alike: the trigger resolves the metered entry
/// in it.
async fn current_conversation(
    conversation_id: &str,
    path: &CompactionPath<'_>,
    stream: &Stream,
    lifecycle: &LifecycleTx,
) -> Option<Conversation> {
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
        Ok(Ok(conversation)) => Some(conversation),
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
pub(crate) async fn run_compaction(
    conversation_id: &str,
    path: &CompactionPath<'_>,
    source: CompactionSource,
    stream: &Stream,
    lifecycle: &LifecycleTx,
) -> Option<Vec<Message>> {
    let CompactionPath {
        memory,
        compaction,
        meter,
        store_latch,
        ..
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

    // One consumer for both sources: the flag marks a request the provider
    // refused as too long, and the compaction that follows — asked for or
    // scheduled — is the one that answers it. Consuming it here is what keeps a
    // stale verdict from making the next decision plan a second, deeper cut.
    let reason = compaction.take_overflowed();
    match compaction
        .compact(store, conversation_id, reason, source)
        .await
    {
        Ok(Some(outcome)) => {
            // Compaction succeeded: the request and the usage it would meter no
            // longer describe the (now summarized) context. Forget both, so the
            // next decision meters the spliced conversation whole.
            meter.reset();
            Some(outcome.messages)
        }
        Ok(None) => {
            let _ = stream.publish(SessionRecord::Compaction(CompactionRecord::Completed {
                source: Some(source),
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

// region:    --- Support

// endregion: --- Support
