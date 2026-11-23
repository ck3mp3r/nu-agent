//! The recording sinks and the replayed-update readers.

use super::*;

/// Test double for [`SessionUpdateSink`]: records every notification.
#[derive(Default)]
pub(crate) struct RecordingSink {
    updates: Mutex<Vec<SessionUpdate>>,
    fail: bool,
}

impl RecordingSink {
    pub(crate) fn failing() -> Self {
        Self {
            updates: Mutex::new(Vec::new()),
            fail: true,
        }
    }

    pub(crate) fn taken(&self) -> Vec<SessionUpdate> {
        self.updates
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }
}

impl SessionUpdateSink for RecordingSink {
    fn send_update(
        &self,
        _session_id: &str,
        update: SessionUpdate,
    ) -> core::result::Result<(), AcpError> {
        if self.fail {
            return Err(AcpError::Runtime("transport closed".to_string()));
        }
        if let Ok(mut guard) = self.updates.lock() {
            guard.push(update);
        }
        Ok(())
    }
}

/// The text of a replayed message chunk, or `None` for any other update.
pub(crate) fn chunk_text(update: &SessionUpdate) -> Option<String> {
    let chunk = match update {
        SessionUpdate::UserMessageChunk(chunk) | SessionUpdate::AgentMessageChunk(chunk) => chunk,
        _ => return None,
    };
    match &chunk.content {
        ContentBlock::Text(text) => Some(text.text.clone()),
        _ => None,
    }
}

/// The prompt tests never reach a tool call, so this sink is never asked; it
/// answers `Cancelled` if it ever is.
impl PermissionRequestSink for RecordingSink {
    async fn send_request(
        &self,
        _session_id: &str,
        _tool_call: ToolCallUpdate,
        _options: Vec<PermissionOption>,
    ) -> RequestPermissionOutcome {
        RequestPermissionOutcome::Cancelled
    }
}
