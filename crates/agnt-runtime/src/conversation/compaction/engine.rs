//! The one model call every compaction path makes: stream a request, report the
//! deltas its caller wants reported, and validate the answer as compaction text.
//!
//! Three paths run a call — the manual compactor, the side-call fallback, and the
//! notes call — so the streaming loop and its rejections live here once.

use futures::StreamExt;

use agnt_kernel::bus::{LifecycleEvent, NarrationEvent};
use agnt_kernel::{CompactionRecord, CompactionSource, SessionRecord};

use rig::DynModel;
use rig::completion::CompletionRequest;
use rig::operation::Completion;
use rig_memory::Summarizer;

use crate::bus::{CancelTx, LifecycleTx, NarrationTx, Stream};
use crate::session::MemoryError;

// region:    --- Types

/// Whether a call's text deltas reach the surfaces.
///
/// A summary streams into the transcript's compaction block; the notes on
/// cleared outputs do not — they are written to the store, and no block draws
/// them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Chunks {
    /// Publish `SummaryChunk` per text delta.
    Publish,
    /// Publish nothing per delta.
    Silent,
}

/// The channels a compaction call publishes on, taken one by one: a component
/// takes the channels it needs, never the whole hub.
#[derive(Clone, Copy)]
pub(super) struct Wires<'a> {
    pub stream: &'a Stream,
    pub narration: &'a NarrationTx,
    pub lifecycle: &'a LifecycleTx,
    pub cancel: &'a CancelTx,
}

// endregion: --- Types

// region:    --- Functions

/// Stream `request` to `model` and return its validated text.
///
/// `chunks` decides whether the text deltas reach the surfaces. Every failure —
/// the stream does not open, a stream item fails, the user cancels, or the folded
/// answer cannot be compaction text — publishes `CompactionRecord::Failed` and
/// returns the error.
pub(super) async fn stream_text(
    wires: Wires<'_>,
    model: &DynModel<Completion>,
    request: CompletionRequest,
    source: CompactionSource,
    chunks: Chunks,
) -> Result<String, MemoryError> {
    // `DynModel::stream` opens the reply synchronously; a failure to open
    // (transport, auth, malformed request) surfaces here rather than as a
    // stream item.
    let stream = match model.stream(request) {
        Ok(stream) => stream,
        Err(e) => {
            let message = format!("Compaction stream error: {e}");
            emit_failed(wires, source, &message).await;
            return Err(MemoryError::backend(message));
        }
    };

    let mut stream = std::pin::pin!(stream);
    let mut aggregated = String::new();
    let mut cancel_rx = wires.cancel.subscribe();

    loop {
        tokio::select! {
            item = stream.next() => {
                match item {
                    Some(Ok(chunk)) => match chunk {
                        rig::streaming::Item::Event(rig::streaming::StreamEvent::Text {
                            text,
                            ..
                        }) => {
                            aggregated.push_str(&text);
                            if chunks == Chunks::Publish {
                                let _ = wires.stream.publish(SessionRecord::Compaction(
                                    CompactionRecord::SummaryChunk {
                                        source,
                                        delta: text.clone(),
                                        aggregated: aggregated.clone(),
                                    },
                                ));
                                let _ = wires.narration.publish(
                                    NarrationEvent::CompactionSummaryChunk {
                                        source,
                                        delta: text,
                                        aggregated: aggregated.clone(),
                                    },
                                );
                            }
                        }
                        rig::streaming::Item::Event(rig::streaming::StreamEvent::Reasoning {
                            ..
                        }) => {
                            // Reasoning is not summary content. Log it for
                            // observability but do not append it to the summary.
                            log::debug!(
                                "Compaction stream produced reasoning chunk (not summary content): {chunk:?}"
                            );
                        }
                        rig::streaming::Item::Event(rig::streaming::StreamEvent::Start {
                            ..
                        }) => {
                            log::trace!("Compaction stream part started: {chunk:?}");
                        }
                        rig::streaming::Item::Event(rig::streaming::StreamEvent::End {
                            content: crate::types::AssistantContent::ToolCall(_),
                            ..
                        })
                        | rig::streaming::Item::Event(rig::streaming::StreamEvent::Arguments {
                            ..
                        }) => {
                            let message = "Unexpected tool call during compaction".to_string();
                            emit_failed(wires, source, &message).await;
                            return Err(MemoryError::backend(message));
                        }
                        rig::streaming::Item::Event(rig::streaming::StreamEvent::End {
                            ..
                        }) => {
                            // The finalized part content is not summary text.
                            // Provider usage is read from the stream after the loop.
                        }
                        rig::streaming::Item::Unknown(_) => {
                            let message = "Unexpected stream chunk: unknown".to_string();
                            emit_failed(wires, source, &message).await;
                            return Err(MemoryError::backend(message));
                        }
                    },
                    Some(Err(e)) => {
                        let message = format!("Compaction stream error: {e}");
                        emit_failed(wires, source, &message).await;
                        return Err(MemoryError::backend(message));
                    }
                    None => break,
                }
            }
            Ok(_) = cancel_rx.recv() => {
                let message = "Compaction cancelled by user".to_string();
                emit_failed(wires, source, &message).await;
                return Err(MemoryError::backend(message));
            }
        }
    }

    // The provider's usage arrives on the folded response, which `partial()`
    // exposes once the stream ended. `summary_text` validates the folded
    // answer: a truncated, tool-calling, or empty answer is refused.
    match Summarizer::summary_text(&stream.partial()) {
        Ok(text) => Ok(text),
        Err(e) => {
            let message = format!("Compaction produced an unusable summary: {e}");
            emit_failed(wires, source, &message).await;
            Err(MemoryError::backend(message))
        }
    }
}

/// Emit the compaction pipeline's `Completed` rows for `summary`.
pub(super) async fn emit_completed(wires: Wires<'_>, summary: &str, source: CompactionSource) {
    let _ = wires
        .stream
        .publish(SessionRecord::Compaction(CompactionRecord::Completed {
            source,
            summary: summary.to_string(),
        }));
    let _ = wires
        .lifecycle
        .publish(LifecycleEvent::CompactionCompleted {
            source,
            summary: summary.to_string(),
        });
}

/// Emit the compaction pipeline's `Started` rows.
pub(super) async fn emit_started(wires: Wires<'_>, source: CompactionSource) {
    let _ = wires
        .stream
        .publish(SessionRecord::Compaction(CompactionRecord::Started {
            source,
        }));
    let _ = wires
        .lifecycle
        .publish(LifecycleEvent::CompactionStarted { source });
}

/// Emit `CompactionRecord::Failed` carrying `message`.
pub(super) async fn emit_failed(wires: Wires<'_>, source: CompactionSource, message: &str) {
    let _ = wires
        .stream
        .publish(SessionRecord::Compaction(CompactionRecord::Failed {
            source,
            message: message.to_string(),
        }));
    let _ = wires.lifecycle.publish(LifecycleEvent::CompactionFailed {
        source,
        message: message.to_string(),
    });
}

// endregion: --- Functions
