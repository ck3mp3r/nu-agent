//! Translation of nu-agent session records into ACP `session/update` notifications.
//!
//! The ACP prompt handler drains the session bus while `execute_turn()` runs and
//! forwards every translated event to the client as a `session/update`
//! notification. Events without an ACP equivalent are dropped.
//!
//! Translation is stateful: the translator allocates its own ACP tool-call id
//! per started call and matches the completion back to it, so the ids a client
//! sees keep their `{name}-{seq}` shape regardless of the call key the event
//! carries. One [`EventTranslator`] therefore serves one session.
//!
//! The translator is split by topic: `types` holds the vocabulary and the
//! translator's state, `event_translator` the record-to-ACP methods, `drain` the
//! loop drivers, `wire` the free-standing schema converters, and `support` the
//! drain plumbing.

// region:    --- Modules

mod drain;
mod event_translator;
mod support;
mod types;
mod wire;

pub use drain::{
    drain_bus_to_notifications, drain_receiver_to_notifications, flush_receiver_to_notifications,
    run_turn_with_notifications,
};
pub use types::{EventTranslator, SessionUpdateSink, TranslateInput, TurnSources};

pub(crate) use wire::content_from_display;

// endregion: --- Modules

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/translator.rs"]
mod translator_test;

// endregion: --- Tests
