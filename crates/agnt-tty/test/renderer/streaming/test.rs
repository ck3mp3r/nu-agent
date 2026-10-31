//! Stderr renderer streaming tests.
//!
//! The suite is split into topical sub-files. Shared imports live here; each
//! sub-file pulls them in with `use super::*;`.

// region:    --- Modules

#[path = "compaction.rs"]
mod compaction;

#[path = "stopped.rs"]
mod stopped;

#[path = "text.rs"]
mod text;

#[path = "tool_and_usage.rs"]
mod tool_and_usage;

#[path = "verbosity.rs"]
mod verbosity;

// endregion: --- Modules

use agnt_core::protocol::events::{SystemEvent, SystemKind, ToolCall, ToolOutcome, Usage};
use agnt_core::protocol::tool_args::CallLine;
use agnt_core::renderer::UiRenderer;
use agnt_core::transcript::ir::ToolName;

use super::StderrUiRenderer;
use crate::policy::{UiPolicy, Verbosity};

pub(super) type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

// region:    --- Test Support

/// A writer the test keeps a handle on, so the printed bytes can be read
/// between chunks instead of only after the renderer is dropped.
#[derive(Clone, Default)]
pub(super) struct SharedSink(std::rc::Rc<std::cell::RefCell<Vec<u8>>>);

impl SharedSink {
    pub(super) fn new() -> Self {
        Self::default()
    }

    /// Everything written so far, as text.
    pub(super) fn snapshot(&self) -> String {
        String::from_utf8(self.0.borrow().clone()).unwrap_or_default()
    }
}

impl std::io::Write for SharedSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.borrow_mut().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

// endregion: --- Test Support
