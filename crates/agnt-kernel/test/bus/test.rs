//! Contract tests for the two-ring substrate, split by concern — mirroring
//! `src/bus/`.

// region:    --- Modules

#[path = "agent.rs"]
mod agent;
#[path = "cancel.rs"]
mod cancel;
#[path = "channel.rs"]
mod channel;
#[path = "stream.rs"]
mod stream;

// endregion: --- Modules
