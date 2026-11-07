//! The transcript record: one persisted row of a session's transcript, and the
//! identity it derives from its own content.
//!
//! The store owns the transcript fold (D19). The kernel's stream records are the
//! fold's input; this record is its output, so the persisted shape does not move
//! when the kernel's record vocabulary does: no field here names an `agnt-kernel`
//! symbol.
//!
//! The record carries data only. How a row is drawn — the call line, the
//! sections, the mark of a call that produced no result — is the drawer's
//! derivation from these fields, never a field here.

// region:    --- Modules

mod identity;
mod record;

pub use identity::TranscriptId;
pub use record::{
    MediaKind, MediaOwner, MediaSource, TranscriptRecord, TranscriptRow, TranscriptStamp,
};

// endregion: --- Modules

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/transcript.rs"]
mod transcript_test;

// endregion: --- Tests
