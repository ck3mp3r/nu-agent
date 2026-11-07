//! The session resolver: which session a run attaches to, what its transcript
//! reads as, and the fold that records it.
//!
//! The suite is split by responsibility. [`resolve`] decides which session to
//! attach or create and opens its store; [`record`] folds the session's log into
//! the store's transcript rows and reads them back; [`hydration`] turns the
//! entries into the transcript a surface replays.

// region:    --- Modules

mod hydration;
mod record;
mod resolve;

#[cfg(test)]
pub(crate) use hydration::hydrate_single_message;
pub use hydration::{StoredEntry, hydrate_transcript_messages};
pub(crate) use record::read_entries;
pub use resolve::{
    DefaultSessionResolver, SessionRequest, SessionResolution, SessionResolutionInput,
    SessionResolver, generate_session_id, resolve_session_request,
};

// endregion: --- Modules
