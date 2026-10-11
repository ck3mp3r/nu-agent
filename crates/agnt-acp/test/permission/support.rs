//! Shared fixtures for the permission bridge test suite.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    PermissionOption, RequestPermissionOutcome, SelectedPermissionOutcome, ToolCallUpdate,
};
use agnt_core::bus::{OneshotRx, OneshotTx};
use agnt_core::conversation::runtime::PendingPermissions;
use agnt_core::protocol::event::{
    PermissionDecision, PermissionRequestContext, ToolDisplay, ToolDisplaySection,
};
use agnt_core::transcript::ir::ContentKind;

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
        self.requests.lock().unwrap().clone()
    }
}

impl PermissionRequestSink for RecordingPermissionSink {
    fn send_request(
        &self,
        session_id: &str,
        tool_call: ToolCallUpdate,
        options: Vec<PermissionOption>,
    ) -> impl Future<Output = RequestPermissionOutcome> + Send {
        self.requests.lock().unwrap().push(RecordedRequest {
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

pub fn context() -> PermissionRequestContext {
    context_for(TOOL_KEY)
}

pub fn context_for(tool_key: &str) -> PermissionRequestContext {
    PermissionRequestContext {
        tool: "edit".to_string(),
        tool_key: tool_key.to_string(),
        source: "builtin".to_string(),
        mode: None,
        matched_rule_identity: "rule".to_string(),
        scope: "global".to_string(),
        target_field: None,
        pattern: "*".to_string(),
        summary: "→ edit src/lib.rs".to_string(),
        pre_authorize_display: None,
    }
}

pub fn context_with_display(display: ToolDisplay) -> PermissionRequestContext {
    let mut context = context();
    context.pre_authorize_display = Some(display);
    context
}

pub fn diff_display() -> ToolDisplay {
    ToolDisplay {
        title: None,
        sections: vec![ToolDisplaySection {
            label: Some("src/lib.rs".to_string()),
            kind: ContentKind::Diff {
                language: "rust".to_string(),
                path: "src/lib.rs".to_string(),
            },
            content: "@@ -1,2 +1,2 @@\n-old line\n+new line\n context\n".to_string(),
            stats: None,
        }],
    }
}

pub fn empty_pending() -> PendingPermissions {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn register_pending(
    pending: &PendingPermissions,
    request_id: &str,
) -> OneshotRx<PermissionDecision> {
    let (tx, rx) = OneshotTx::<PermissionDecision>::channel("permission");
    pending.lock().unwrap().insert(request_id.to_string(), tx);
    rx
}
