//! Renderer contract tests.
//!
//! The trait-substitutability test lives here. The stderr renderer's behavior
//! tests live in the `stderr` sub-file and its subfiles.

#[path = "contract/stderr.rs"]
mod stderr;

use std::sync::{Arc, Mutex};

use crate::renderer::{RenderInput, UiRenderer};
use agnt_kernel::bus::NarrationEvent;
use agnt_kernel::{
    InferenceRecord, NoticeKind, PermissionRecord, SessionRecord, ToolCall, ToolOutcome,
    ToolRecord, ToolStatus, TurnRecord, Usage,
};

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
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

impl UiRenderer for FakeRenderer {
    fn draw(&mut self, input: RenderInput<'_>) {
        let label = match input {
            RenderInput::Record(SessionRecord::Inference(InferenceRecord::Started)) => "llm_start",
            RenderInput::Record(SessionRecord::Inference(InferenceRecord::Completed {
                ..
            })) => "llm_end",
            RenderInput::Record(SessionRecord::Inference(InferenceRecord::Stopped { .. })) => {
                "stopped"
            }
            RenderInput::Record(SessionRecord::Inference(InferenceRecord::Chunk { .. })) => "chunk",
            RenderInput::Record(SessionRecord::Tool(ToolRecord::Started { .. })) => "tool_start",
            RenderInput::Record(SessionRecord::Tool(ToolRecord::Completed { .. })) => "tool_end",
            RenderInput::Record(SessionRecord::Permission(PermissionRecord::Requested {
                ..
            })) => "permission_requested",
            RenderInput::Record(SessionRecord::Permission(PermissionRecord::Decided {
                ..
            })) => "permission_decision_submitted",
            RenderInput::Record(SessionRecord::Permission(PermissionRecord::TimedOut {
                ..
            })) => "permission_decision_timed_out",
            RenderInput::Record(SessionRecord::Permission(PermissionRecord::Ignored {
                ..
            })) => "permission_decision_ignored",
            RenderInput::Record(SessionRecord::Permission(PermissionRecord::Cancelled {
                ..
            })) => "permission_request_cancelled",
            RenderInput::Record(SessionRecord::Permission(PermissionRecord::AutoResolved {
                ..
            })) => "permission_auto_resolved",
            RenderInput::Record(SessionRecord::Compaction(_)) => "compaction",
            RenderInput::Record(SessionRecord::Session(_)) => "session",
            RenderInput::Record(SessionRecord::Prompt(_)) => "prompt",
            RenderInput::Record(SessionRecord::Notice(_)) => "notice",
            RenderInput::Record(SessionRecord::Turn(TurnRecord::Completed { .. })) => "completed",
            RenderInput::Narration(NarrationEvent::SystemMessage {
                kind: NoticeKind::Warning,
                ..
            }) => "warning",
            RenderInput::Narration(NarrationEvent::SystemMessage {
                kind: NoticeKind::TurnError,
                ..
            }) => "turn_error",
            RenderInput::Narration(NarrationEvent::InferenceChunk { .. }) => "assistant_message",
            RenderInput::Narration(_) => "narration",
            RenderInput::Lifecycle(_) => "lifecycle",
        };
        self.events
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(label.to_string());
    }

    fn flush(&mut self) {}
}

fn tool_call(name: &str) -> ToolCall {
    ToolCall::new(
        name,
        agnt_runtime::tools::handler::ToolSource::Closure,
        "{}",
    )
}

/// Draw one record through `renderer`, so every fixture reads the same.
fn draw<R: UiRenderer>(renderer: &mut R, record: SessionRecord) {
    renderer.draw(RenderInput::Record(&record));
}

/// Draw one narration datum through `renderer`.
fn narrate<R: UiRenderer>(renderer: &mut R, event: NarrationEvent) {
    renderer.draw(RenderInput::Narration(&event));
}

fn run_mock_flow<R: UiRenderer>(renderer: &mut R) {
    draw(renderer, SessionRecord::Inference(InferenceRecord::Started));
    draw(
        renderer,
        SessionRecord::Inference(InferenceRecord::Completed {
            usage: Usage {
                input_tokens: 2,
                output_tokens: 1,
                total_tokens: Some(3),
                cost: None,
                cost_complete: false,
            },
        }),
    );
    draw(
        renderer,
        SessionRecord::Tool(ToolRecord::Started {
            call: tool_call("t"),
        }),
    );
    draw(
        renderer,
        SessionRecord::Tool(ToolRecord::Completed {
            call: tool_call("t"),
            outcome: ToolOutcome {
                status: ToolStatus::Success,
                payload: None,
                text: "ok".to_string(),
                message: None,
                kind: None,
            },
        }),
    );
    draw(
        renderer,
        SessionRecord::Turn(TurnRecord::Completed { duration_ms: 0 }),
    );
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
            "llm_end",
            "tool_start",
            "tool_end",
            "completed"
        ]
    );
}
