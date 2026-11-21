//! The record vocabulary's classification for the stream arm's instrument.
//!
//! A record's kind label and its carried bytes are read-only facts about the
//! record, so they live beside the fold that dispatches it — never in the loop
//! that measures. The bytes come from `String::len()` fields alone: measuring a
//! fold must never scan its payload.

use agnt_runtime::protocol::events::{
    CompactionRecord, InferenceRecord, NarrationEvent, SessionRecord, ToolRecord,
};

/// The kind label a stream record folds under: the arm's per-kind totals are
/// keyed by this name, so every distinct fold step shows in the warn line.
pub(crate) fn record_kind_label(record: &SessionRecord) -> &'static str {
    match record {
        SessionRecord::Prompt(_) => "prompt",
        SessionRecord::Inference(InferenceRecord::Chunk { .. }) => "inference-chunk",
        SessionRecord::Inference(InferenceRecord::Completed { .. }) => "inference-completed",
        SessionRecord::Inference(InferenceRecord::Stopped { .. }) => "inference-stopped",
        SessionRecord::Compaction(CompactionRecord::SummaryChunk { .. }) => {
            "compaction-summary-chunk"
        }
        SessionRecord::Tool(ToolRecord::Started { .. }) => "tool-started",
        SessionRecord::Tool(ToolRecord::Completed { .. }) => "tool-completed",
        SessionRecord::Notice(_) => "notice",
        SessionRecord::Turn(_) => "turn",
        SessionRecord::Session(_) => "session",
        SessionRecord::Permission(_) => "permission",
        _ => "other",
    }
}

/// The bytes a record carries, from its `String::len()` fields alone.
pub(crate) fn record_payload_bytes(record: &SessionRecord) -> u128 {
    let bytes = match record {
        SessionRecord::Prompt(prompt) => prompt.text.len(),
        SessionRecord::Inference(InferenceRecord::Chunk { aggregated, .. }) => aggregated.len(),
        SessionRecord::Inference(InferenceRecord::Stopped { reason, .. }) => reason.len(),
        SessionRecord::Compaction(CompactionRecord::SummaryChunk { aggregated, .. }) => {
            aggregated.len()
        }
        SessionRecord::Compaction(CompactionRecord::Completed { summary, .. }) => summary.len(),
        SessionRecord::Compaction(CompactionRecord::Failed { message, .. }) => message.len(),
        SessionRecord::Tool(ToolRecord::Started { call }) => call.arguments.len(),
        SessionRecord::Tool(ToolRecord::Completed { call, .. }) => call.arguments.len(),
        _ => 0,
    };
    bytes as u128
}

/// The kind label a narration event folds under.
pub(crate) fn narration_kind_label(event: &NarrationEvent) -> &'static str {
    match event {
        NarrationEvent::InferenceChunk { .. } => "inference-chunk",
        NarrationEvent::InferenceStarted => "inference-started",
        NarrationEvent::InferenceCompleted { .. } => "inference-completed",
        NarrationEvent::InferenceStopped { .. } => "inference-stopped",
        NarrationEvent::CompactionSummaryChunk { .. } => "compaction-summary-chunk",
        NarrationEvent::SystemMessage { .. } => "system-message",
    }
}

/// The bytes a narration event carries, from its `String::len()` fields alone.
pub(crate) fn narration_payload_bytes(event: &NarrationEvent) -> u128 {
    let bytes = match event {
        NarrationEvent::InferenceChunk { aggregated, .. } => aggregated.len(),
        NarrationEvent::CompactionSummaryChunk { aggregated, .. } => aggregated.len(),
        NarrationEvent::InferenceStopped { reason } => reason.len(),
        NarrationEvent::SystemMessage { text, .. } => text.len(),
        _ => 0,
    };
    bytes as u128
}
