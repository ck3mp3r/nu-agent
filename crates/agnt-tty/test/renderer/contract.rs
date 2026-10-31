//! Renderer contract tests.
//!
//! The trait-substitutability test lives here. The stderr renderer's behavior
//! tests live in the `stderr` sub-file and its subfiles.

#[path = "contract/stderr.rs"]
mod stderr;

use std::sync::{Arc, Mutex};

use agnt_core::protocol::events::{SystemEvent, SystemKind, ToolCall, ToolOutcome, Usage};
use agnt_core::protocol::tool_args::CallLine;
use agnt_core::renderer::UiRenderer;
use agnt_core::session::tool_status::ToolStatus;
use agnt_core::transcript::ir::ToolName;

use crate::policy::UiPolicy;

#[derive(Clone)]
struct FakeRenderer {
    events: Arc<Mutex<Vec<String>>>,
}

impl FakeRenderer {
    fn new(_policy: UiPolicy) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().expect("events").clone()
    }
}

impl UiRenderer for FakeRenderer {
    fn emit(&mut self, event: &SystemEvent) {
        let label = match event {
            SystemEvent::InferenceStarted => "llm_start",
            SystemEvent::Tick => "tick",
            SystemEvent::InferenceCompleted { .. } => "llm_end",
            SystemEvent::ToolStarted { .. } => "tool_start",
            SystemEvent::ToolCompleted { .. } => "tool_end",
            SystemEvent::PreviewRequested { .. } => "tool_preview",
            SystemEvent::PermissionRequested { .. } => "permission_requested",
            SystemEvent::PermissionDecisionSubmitted { .. } => "permission_decision_submitted",
            SystemEvent::PermissionDecisionTimedOut { .. } => "permission_decision_timed_out",
            SystemEvent::PermissionDecisionIgnored { .. } => "permission_decision_ignored",
            SystemEvent::PermissionRequestCancelled { .. } => "permission_request_cancelled",
            SystemEvent::PermissionAutoResolved { .. } => "permission_auto_resolved",
            SystemEvent::SystemMessage {
                kind: SystemKind::Warning,
                ..
            } => "warning",
            SystemEvent::SystemMessage {
                kind: SystemKind::TurnError,
                ..
            } => "turn_error",
            SystemEvent::CompactionStarted { .. } => "compaction_started",
            SystemEvent::CompactionSummaryChunk { .. } => "compaction_summary_chunk",
            SystemEvent::CompactionCompleted { .. } => "compaction_completed",
            SystemEvent::CompactionFailed { .. } => "compaction_failed",
            SystemEvent::InferenceChunk { .. } => "assistant_message",
            SystemEvent::InferenceStopped { .. } => "stopped",
            SystemEvent::TurnCompleted { .. } => "completed",
            SystemEvent::KeyPressed { .. }
            | SystemEvent::WheelScrolled { .. }
            | SystemEvent::Resized { .. }
            | SystemEvent::PermissionDecided { .. }
            | SystemEvent::PromptSubmitted { .. }
            | SystemEvent::CompactionRequested { .. } => "unrendered",
        };
        self.events.lock().expect("events").push(label.to_string());
    }

    fn flush(&mut self) {}
}

fn tool_call(name: &str) -> ToolCall {
    ToolCall::new(
        ToolName(name.to_string()),
        "closure",
        "{}",
        CallLine::from_json_summary("{}"),
    )
}

fn run_mock_flow<R: UiRenderer>(renderer: &mut R) {
    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::Tick);
    renderer.emit(&SystemEvent::InferenceCompleted {
        usage: Usage {
            response_chars: 3,
            tool_calls: 1,
            input_tokens: 2,
            output_tokens: 1,
            total_tokens: Some(3),
            cost: None,
            cost_complete: false,
        },
    });
    renderer.emit(&SystemEvent::ToolStarted {
        call: tool_call("t"),
    });
    renderer.emit(&SystemEvent::ToolCompleted {
        call: tool_call("t"),
        outcome: ToolOutcome {
            success: true,
            status: ToolStatus::Success,
            result: "ok".to_string(),
            display: None,
            error_kind: None,
            message: None,
        },
    });
    renderer.emit(&SystemEvent::TurnCompleted {
        tool_calls: 1,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: None,
        cost: None,
        cost_complete: true,
        duration_ms: 0,
    });
    renderer.flush();
}

#[test]
fn renderer_is_substitutable_via_trait_boundary() {
    let mut fake = FakeRenderer::new(UiPolicy {
        quiet: false,
        verbosity: crate::policy::Verbosity::Normal,
    });
    run_mock_flow(&mut fake);
    assert_eq!(
        fake.events(),
        vec![
            "llm_start",
            "tick",
            "llm_end",
            "tool_start",
            "tool_end",
            "completed"
        ]
    );
}
