//! The clipboard seam: the yank's copy, handed to the blocking pool, and the
//! outcome the tick folds.

use super::RuntimeCoordinator;

impl RuntimeCoordinator {
    /// Hand a pending yank to the blocking pool.
    ///
    /// The pasteboard call is blocking OS IPC, so it never runs on the loop
    /// task. One copy is in flight at a time: a request set while one runs stays
    /// pending and starts on the first flush after
    /// [`Self::poll_clipboard_outcome`] folds the outcome.
    pub(super) fn flush_clipboard_request(&mut self) {
        if self.clipboard_copy.is_some() {
            return;
        }
        let Some(text) = self.state.input.take_clipboard_request() else {
            return;
        };
        let writer = self.clipboard_writer;
        self.clipboard_copy = Some(tokio::task::spawn_blocking(move || writer(&text)));
    }

    /// Fold a finished copy, if there is one. Returns whether the fold changed
    /// the rendered state.
    ///
    /// Only a finished handle is awaited, so this never waits: a copy still
    /// running stays pending for a later poll.
    pub(crate) async fn poll_clipboard_outcome(&mut self) -> bool {
        let Some(copy) = self.clipboard_copy.take() else {
            return false;
        };
        if !copy.is_finished() {
            self.clipboard_copy = Some(copy);
            return false;
        }
        let failure = match copy.await {
            Ok(Ok(())) => return false,
            Ok(Err(error)) => error,
            Err(join_error) => join_error.to_string(),
        };
        self.state
            .status
            .message
            .set_message(format!("Clipboard copy failed: {failure}"));
        true
    }
}

/// The production clipboard writer: one blocking pasteboard call.
///
/// Only ever reached through [`RuntimeCoordinator::flush_clipboard_request`],
/// which runs it on the blocking pool — never on the loop task.
pub(super) fn write_clipboard_text(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text))
        .map_err(|error| error.to_string())
}
