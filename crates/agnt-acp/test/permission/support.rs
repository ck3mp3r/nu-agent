//! Shared fixtures for the permission bridge test suite.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    PermissionOption, RequestPermissionOutcome, SelectedPermissionOutcome, ToolCallUpdate,
};
use agnt_kernel::{PermissionRecord, RequestId, ToolCallId};
use agnt_runtime::bus::{OneshotRx, OneshotTx};
use agnt_runtime::conversation::runtime::PendingPermissions;
use agnt_runtime::protocol::event::PermissionDecision;

use crate::permission::PermissionRequestSink;

pub const SESSION: &str = "sess-1";
pub const REQUEST: &str = "req-1";
pub const TOOL_KEY: &str = "edit\n{\"path\":\"src/lib.rs\"}";
pub const OTHER_TOOL_KEY: &str = "edit\n{\"path\":\"src/main.rs\"}";

/// One `session/request_permission` call captured by the mock sink.
#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub session_id: String,
    pub tool_call: ToolCallUpdate,
    pub options: Vec<PermissionOption>,
}

/// Test double for [`PermissionRequestSink`]: records every request and
/// returns a fixed outcome.
#[derive(Clone)]
pub struct RecordingPermissionSink {
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    outcome: RequestPermissionOutcome,
}

impl RecordingPermissionSink {
    pub fn new(outcome: RequestPermissionOutcome) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            outcome,
        }
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl PermissionRequestSink for RecordingPermissionSink {
    fn send_request(
        &self,
        session_id: &str,
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
    ) -> impl Future<Output = RequestPermissionOutcome> + Send {
        self.requests
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(RecordedRequest {
                session_id: session_id.to_string(),
                tool_call,
                options,
            });
        let outcome = self.outcome.clone();
        async move { outcome }
    }
}

pub fn selected(option_id: &'static str) -> RequestPermissionOutcome {
    RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id))
}

/// The card request `REQUEST` opens for `TOOL_KEY`, with no preview.
pub fn card() -> PermissionRecord {
    card_with_request(REQUEST)
}

/// The same card under another request id, for tests that open two cards.
pub fn card_with_request(request_id: &str) -> PermissionRecord {
    card_with_request_for(request_id, TOOL_KEY)
}

/// The same card for another call key, for tests that scope a deny set.
pub fn card_with_request_for(request_id: &str, call_key: &str) -> PermissionRecord {
    PermissionRecord::Requested {
        request_id: RequestId::from_minted(request_id),
        call_id: ToolCallId::from_key(call_key),
        tool: "edit".to_string(),
        source: "builtin".to_string(),
        rule_identity: "rule".to_string(),
        scope: "global".to_string(),
        target_field: None,
        mode: None,
        pattern: "*".to_string(),
        summary: "→ edit src/lib.rs".to_string(),
        shows_preview: false,
        preview: None,
    }
}

/// A card whose preview carries the producer-computed content.
pub fn card_with_preview(content: &str) -> PermissionRecord {
    let mut card = card();
    if let PermissionRecord::Requested {
        shows_preview,
        preview,
        ..
    } = &mut card
    {
        *shows_preview = true;
        *preview = Some(content.to_string());
    }
    card
}

/// The preview content an `edit` gate supplies: the diff it would make.
pub fn diff_content() -> String {
    "@@ -1,2 +1,2 @@\n-old line\n+new line\n context\n".to_string()
}

pub fn empty_pending() -> PendingPermissions {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn register_pending(
    pending: &PendingPermissions,
    request_id: &str,
) -> OneshotRx<PermissionDecision> {
    let (tx, rx) = OneshotTx::<PermissionDecision>::channel("permission");
    pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(request_id.to_string(), tx);
    rx
}
