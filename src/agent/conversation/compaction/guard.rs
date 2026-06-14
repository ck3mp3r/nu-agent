use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// RAII guard that resets the compaction flag when dropped, even on error/panic.
pub(in crate::agent::conversation) struct CompactionGuard(
    pub(in crate::agent::conversation) Arc<AtomicBool>,
);

impl Drop for CompactionGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
