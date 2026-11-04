//! The model ids already reported as unpriced.
//!
//! A tokens-only session is explainable from one warn line per model. Repeating
//! it on every call would bury the log, so the first report is the only one.

use std::collections::HashSet;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// The model ids already reported as unpriced.
///
/// Cloning shares the set, so every clone of a hook reports each model once.
#[derive(Debug, Clone, Default)]
pub struct UnpricedWarnings {
    warned: Arc<Mutex<HashSet<String>>>,
}

impl UnpricedWarnings {
    /// Whether `model_id` is being reported for the first time.
    ///
    /// Returns `false` for a model already reported, so the caller logs once.
    pub fn first_time(&self, model_id: &str) -> bool {
        self.lock_warned().insert(model_id.to_string())
    }

    /// The reported set, recovered if a reporter panicked mid-insert.
    fn lock_warned(&self) -> MutexGuard<'_, HashSet<String>> {
        self.warned.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

// region:    --- Tests

#[cfg(test)]
#[path = "../../test/hook/unpriced_warnings.rs"]
mod unpriced_warnings_test;

// endregion: --- Tests
