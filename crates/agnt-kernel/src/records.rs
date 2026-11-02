//! The session record vocabulary — the typed entries of one ordered stream.
//!
//! A stream record is the log of what happened in a session plus
//! current-truth publications. Records are never commands and never render
//! instructions: surfaces project whichever records they care about.

// region:    --- Types

/// The typed envelope of the session stream. The stream ring delivers these
/// in publish order; the reducer of every consumer folds this vocabulary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionRecord {
    Inference(InferenceRecord),
    Turn(TurnRecord),
}

/// The inference pipeline of one completion call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferenceRecord {
    /// The completion call started.
    Started,
    /// One streamed delta. `aggregated` accumulates from the call start.
    Chunk {
        /// The newly received text.
        delta: String,
        /// The accumulated response text so far.
        aggregated: String,
    },
    /// The completion finished and the totals attached to it arrived.
    Completed { usage: Usage },
}

/// The totals of one inference call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// The total the wire reported, when it reported one.
    pub total_tokens: Option<u64>,
}

/// The turn pipeline of one submitted prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnRecord {
    /// The turn finished; the usage totals arrive on
    /// [`InferenceRecord::Completed`] before this record.
    Completed { tool_calls: usize, duration_ms: u64 },
}

// endregion: --- Types
