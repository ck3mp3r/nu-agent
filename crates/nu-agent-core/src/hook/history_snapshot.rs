//! History snapshot concern.
//!
//! Captures the conversation history just before each LLM HTTP call so that
//! callers can recover it after a `CompletionError`.

use std::sync::{Arc, Mutex};

use rig::message::Message;

/// Captures and exposes a snapshot of the conversation history.
///
/// Updated by the hook's `on_completion_call`. After a `CompletionError`,
/// callers can read the last snapshot to recover the history that was live
/// at the time of the failed LLM call.
#[derive(Clone)]
pub struct HistorySnapshot {
    pub history: Arc<Mutex<Vec<Message>>>,
}

impl HistorySnapshot {
    /// Overwrite the snapshot with `history + [prompt]`.
    pub fn update(&self, history: &[Message], prompt: &Message) {
        let mut snapshot = history.to_vec();
        snapshot.push(prompt.clone());
        *self
            .history
            .lock()
            .expect("history snapshot mutex poisoned") = snapshot;
    }

    /// Return a clone of the Arc holding the snapshot.
    ///
    /// Callers should clone this Arc **before** passing the hook into the agent
    /// builder (which consumes `self`), then read it after a `CompletionError`.
    pub fn arc(&self) -> Arc<Mutex<Vec<Message>>> {
        Arc::clone(&self.history)
    }
}

impl Default for HistorySnapshot {
    fn default() -> Self {
        Self {
            history: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

#[cfg(test)]
#[path = "history_snapshot_test.rs"]
mod history_snapshot_test;
