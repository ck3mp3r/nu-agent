//! Shared fixtures for the translator test suite.

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::v1::{
    PermissionOption, RequestPermissionOutcome, SelectedPermissionOutcome, SessionUpdate,
    ToolCallUpdate,
};
use agnt_core::bus::{OneshotRx, OneshotTx};
use agnt_core::conversation::runtime::PendingPermissions;
use agnt_core::protocol::event::{
    PermissionDecision, PermissionRequestContext, ToolDisplaySection, UiEvent,
};
use agnt_core::protocol::tool_args::CallLine;
use agnt_core::transcript::ir::ContentKind;

use crate::error::AcpError;
use crate::permission::{AcpPermissionBridge, PermissionRequestSink};
use crate::translator::{EventTranslator, SessionUpdateSink};

pub const SESSION: &str = "sess-1";

pub fn translator() -> EventTranslator {
    EventTranslator::new(SESSION)
}

pub fn call_line(summary: &str) -> CallLine {
    CallLine {
        summary: summary.to_string(),
    }
}

pub fn text_section(label: &str, content: &str) -> ToolDisplaySection {
    ToolDisplaySection {
        label: Some(label.to_string()),
        kind: ContentKind::Plain,
        content: content.to_string(),
        stats: None,
    }
}

pub fn diff_section(label: &str, content: &str) -> ToolDisplaySection {
    ToolDisplaySection {
        label: Some(label.to_string()),
        kind: ContentKind::Diff {
            language: "diff".to_string(),
            path: label.to_string(),
        },
        content: content.to_string(),
        stats: None,
    }
}

pub fn tool_started(name: &str, arguments: &str) -> UiEvent {
    UiEvent::ToolStarted {
        name: name.to_string(),
        source: "builtin".to_string(),
        arguments: arguments.to_string(),
        call_line: call_line("→ read src/lib.rs"),
    }
}

pub fn tool_completed(name: &str, arguments: &str, success: bool) -> UiEvent {
    UiEvent::ToolCompleted {
        name: name.to_string(),
        source: "builtin".to_string(),
        arguments: arguments.to_string(),
        success,
        result: "{}".to_string(),
        display: None,
        error_kind: None,
        message: None,
    }
}

pub fn permission_context() -> PermissionRequestContext {
    PermissionRequestContext {
        tool: "edit".to_string(),
        tool_key: "edit\n{}".to_string(),
        source: "builtin".to_string(),
        mode: None,
        matched_rule_identity: "rule".to_string(),
        scope: "global".to_string(),
        target_field: None,
        pattern: "*".to_string(),
        summary: "→ edit".to_string(),
        pre_authorize_display: None,
    }
}

/// One `session/request_permission` call captured by the mock sink.
#[derive(Debug, Clone)]
pub struct RecordedPermissionRequest {
    pub session_id: String,
    pub tool_call: ToolCallUpdate,
    pub options: Vec<PermissionOption>,
}

/// Test double for [`PermissionRequestSink`]: records every request and
/// answers with a fixed outcome.
#[derive(Clone)]
pub struct RecordingPermissionSink {
    requests: Arc<Mutex<Vec<RecordedPermissionRequest>>>,
    outcome: RequestPermissionOutcome,
}

impl RecordingPermissionSink {
    pub fn new(outcome: RequestPermissionOutcome) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            outcome,
        }
    }

    pub fn requests(&self) -> Vec<RecordedPermissionRequest> {
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
        self.requests
            .lock()
            .unwrap()
            .push(RecordedPermissionRequest {
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

pub fn empty_pending() -> PendingPermissions {
    Arc::new(Mutex::new(HashMap::new()))
}

pub fn permission_bridge(
    outcome: RequestPermissionOutcome,
    pending: PendingPermissions,
) -> AcpPermissionBridge<RecordingPermissionSink> {
    AcpPermissionBridge::new(RecordingPermissionSink::new(outcome), SESSION, pending)
}

pub fn register_pending(
    pending: &PendingPermissions,
    request_id: &str,
) -> OneshotRx<PermissionDecision> {
    let (tx, rx) = OneshotTx::<PermissionDecision>::channel("permission");
    pending.lock().unwrap().insert(request_id.to_string(), tx);
    rx
}

/// Test double for [`SessionUpdateSink`]: records every notification.
#[derive(Default)]
pub struct RecordingSink {
    updates: Mutex<Vec<SessionUpdate>>,
    fail: bool,
    updated: tokio::sync::Notify,
}

impl RecordingSink {
    pub fn failing() -> Self {
        Self {
            updates: Mutex::new(Vec::new()),
            fail: true,
            updated: tokio::sync::Notify::new(),
        }
    }

    pub fn taken(&self) -> Vec<SessionUpdate> {
        self.updates
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// Awaits until at least `count` updates have been recorded, so tests can
    /// wait for the drain to forward an event instead of sleeping.
    pub async fn wait_for_updates(&self, count: usize) {
        while self.taken().len() < count {
            self.updated.notified().await;
        }
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
        self.updated.notify_one();
        Ok(())
    }
}
