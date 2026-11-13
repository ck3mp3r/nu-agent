//! The bounded in-memory ring beside the file: the last lines a run produced,
//! kept until a quit or a panic appends them to the file.

use std::collections::VecDeque;
use std::sync::Mutex as StdMutex;

use super::FileLogSink;

/// Number of recent log lines the process ring keeps.
pub(super) const RING_CAPACITY: usize = 500;

/// A bounded buffer of the most recent formatted log lines.
pub(super) struct LogRing {
    pub(super) lines: StdMutex<VecDeque<String>>,
}

impl Default for LogRing {
    fn default() -> Self {
        Self {
            lines: StdMutex::new(VecDeque::with_capacity(RING_CAPACITY)),
        }
    }
}

impl LogRing {
    // --- Mutators ---

    /// Record `line`, dropping the oldest when the ring is full.
    pub(super) fn push(&self, line: &str) {
        let mut lines = self.lock();
        if lines.len() == RING_CAPACITY {
            lines.pop_front();
        }
        lines.push_back(line.to_string());
    }

    /// Append the buffered lines and the dump marker to `sink`, then sync it.
    ///
    /// The ring empties as it dumps, so a quit followed by a panic does not
    /// write the same lines twice. An empty ring writes nothing at all.
    ///
    /// The dump goes through the sink's unsilenced write: a run under `off`
    /// keeps its file silent line by line, but the dump is the evidence it was
    /// recording *for*, so it lands regardless of the resolved level.
    pub(super) fn flush_into(&self, sink: &FileLogSink) {
        let lines = {
            let mut ring = self.lock();
            std::mem::take(&mut *ring)
        };
        if lines.is_empty() {
            return;
        }
        let count = lines.len();
        let mut dump = String::new();
        for line in lines {
            dump.push_str(&line);
            dump.push('\n');
        }
        dump.push_str(&format!("---- ring dump (last {count} lines) ----\n"));
        let _ = sink.write_unsilenced(dump.as_bytes());
    }

    // --- Support ---

    /// Take the ring lock, ignoring poisoning: a dump must never panic.
    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<String>> {
        self.lines.lock().unwrap_or_else(|error| error.into_inner())
    }
}
