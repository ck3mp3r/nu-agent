//! The agent snapshot every hook envelope is stamped with.

use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

// region:    --- Types

/// A shared snapshot of the agent that produces hook events.
///
/// The entry point creates one handle and clones it into the runtime and into
/// every hook runner task. The runtime writes the persona slug and the active
/// model identity on construction and on every persona or model switch; the
/// runners read both when they build an envelope.
#[derive(Clone, Default)]
pub struct HookAgentContext {
    snapshot: Arc<RwLock<HookAgentSnapshot>>,
}

/// The values a hook envelope is stamped with.
#[derive(Default)]
struct HookAgentSnapshot {
    persona: Option<String>,
    model: String,
}

// endregion: --- Types

// region:    --- HookAgentContext Impl

impl HookAgentContext {
    /// Set the persona slug the envelopes carry.
    pub fn with_persona(self, persona: Option<String>) -> Self {
        self.set_persona(persona);
        self
    }

    /// Set the active model identity the envelopes carry.
    pub fn with_model(self, model: String) -> Self {
        self.set_model(model);
        self
    }

    /// Replace the persona slug. `None` means no persona is loaded.
    pub fn set_persona(&self, persona: Option<String>) {
        self.write().persona = persona;
    }

    /// Replace the active model identity.
    pub fn set_model(&self, model: String) {
        self.write().model = model;
    }

    /// The persona slug, or `None` when no persona is loaded.
    pub fn persona(&self) -> Option<String> {
        self.read().persona.clone()
    }

    /// The active model identity, empty when no runtime has set one yet.
    pub fn model(&self) -> String {
        self.read().model.clone()
    }

    /// Read the snapshot, ignoring a poisoned lock: the values are plain data,
    /// so a panic elsewhere never leaves them inconsistent.
    fn read(&self) -> RwLockReadGuard<'_, HookAgentSnapshot> {
        self.snapshot
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Write the snapshot, ignoring a poisoned lock.
    fn write(&self) -> RwLockWriteGuard<'_, HookAgentSnapshot> {
        self.snapshot
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

// endregion: --- HookAgentContext Impl
