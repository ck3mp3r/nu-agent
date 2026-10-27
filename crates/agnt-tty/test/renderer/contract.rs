//! Renderer contract tests.
//!
//! The trait-substitutability test lives here. The stderr renderer's behavior
//! tests live in the `stderr` sub-file and its subfiles.

#[path = "contract/stderr.rs"]
mod stderr;

use std::sync::{Arc, Mutex};

use agnt_core::protocol::event::UiEvent;
use agnt_core::renderer::UiRenderer;

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
    fn emit(&mut self, event: &UiEvent) {
        let label = match event {
            UiEvent::LlmStarted => "llm_start",
            UiEvent::Tick => "tick",
            UiEvent::LlmCompleted { .. } => "llm_end",
            UiEvent::ToolStarted { .. } => "tool_start",
            UiEvent::ToolCompleted { .. } => "tool_end",
            UiEvent::ToolPreview { .. } => "tool_preview",
            UiEvent::PermissionRequested { .. } => "permission_requested",
            UiEvent::PermissionDecisionSubmitted { .. } => "permission_decision_submitted",
            UiEvent::PermissionDecisionTimedOut { .. } => "permission_decision_timed_out",
            UiEvent::PermissionDecisionIgnored { .. } => "permission_decision_ignored",
            UiEvent::PermissionRequestCancelled { .. } => "permission_request_cancelled",
            UiEvent::PermissionAutoResolved { .. } => "permission_auto_resolved",
            UiEvent::Warning { .. } => "warning",
            UiEvent::TurnError { .. } => "turn_error",
            UiEvent::CompactionStarted { .. } => "compaction_started",
            UiEvent::CompactionSummaryChunk { .. } => "compaction_summary_chunk",
            UiEvent::CompactionCompleted { .. } => "compaction_completed",
            UiEvent::CompactionFailed { .. } => "compaction_failed",
            UiEvent::AssistantMessage { .. } => "assistant_message",
            UiEvent::Stopped { .. } => "stopped",
            UiEvent::Completed { .. } => "completed",
        };
        self.events.lock().expect("events").push(label.to_string());
    }

    fn flush(&mut self) {}
}

fn run_mock_flow<R: UiRenderer>(renderer: &mut R) {
    renderer.emit(&UiEvent::LlmStarted);
    renderer.emit(&UiEvent::Tick);
    renderer.emit(&UiEvent::LlmCompleted {
        response_chars: 3,
        tool_calls: 1,
        input_tokens: 2,
        output_tokens: 1,
        total_tokens: Some(3),
        cost: None,
        cost_complete: false,
    });
    renderer.emit(&UiEvent::ToolStarted {
        name: "t".to_string(),
        source: "closure".to_string(),
        arguments: "{}".to_string(),
        call_line: agnt_core::protocol::tool_args::CallLine::from_json_summary("{}"),
    });
    renderer.emit(&UiEvent::ToolCompleted {
        name: "t".to_string(),
        source: "closure".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: "ok".to_string(),
        display: None,
        error_kind: None,
        message: None,
    });
    renderer.emit(&UiEvent::Completed {
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
