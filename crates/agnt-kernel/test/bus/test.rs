//! Contract tests for the bus concerns, split by concern — mirroring
//! `src/bus/`.

// region:    --- Modules

#[path = "agent.rs"]
mod agent;
#[path = "cancel.rs"]
mod cancel;
#[path = "channel.rs"]
mod channel;
#[path = "control.rs"]
mod control;
#[path = "facts.rs"]
mod facts;
#[path = "lifecycle.rs"]
mod lifecycle;
#[path = "narration.rs"]
mod narration;
#[path = "stream.rs"]
mod stream;

// endregion: --- Modules
