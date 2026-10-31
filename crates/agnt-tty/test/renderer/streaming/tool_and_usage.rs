//! Tool display and token-usage lines.

use super::*;

use agnt_core::session::tool_status::ToolStatus;

#[test]
fn tool_display_renders_diff_with_stats() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );
    let display = agnt_core::protocol::event::ToolDisplay {
        title: Some("edit file".to_string()),
        sections: vec![agnt_core::protocol::event::ToolDisplaySection {
            label: Some("diff".to_string()),
            kind: agnt_core::transcript::ir::ContentKind::Diff {
                language: "diff".to_string(),
                path: "test.rs".to_string(),
            },
            content: "+added\n-removed\nunchanged".to_string(),
            stats: Some(agnt_core::protocol::event::ToolDisplayStats {
                files_changed: Some(2),
                insertions: Some(3),
                deletions: Some(1),
                ..Default::default()
            }),
        }],
    };
    renderer.emit(&SystemEvent::ToolCompleted {
        call: ToolCall::new(
            ToolName("edit".to_string()),
            "code",
            "{}",
            CallLine::from_json_summary("{}"),
        ),
        outcome: ToolOutcome {
            success: true,
            status: ToolStatus::Success,
            result: "ok".to_string(),
            display: Some(display),
            error_kind: None,
            message: None,
        },
    });
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("2 files changed"));
}

#[test]
fn llm_end_prints_token_usage() {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );
    renderer.emit(&SystemEvent::InferenceStarted);
    renderer.emit(&SystemEvent::InferenceChunk {
        delta: "hi".to_string(),
        aggregated: "hi".to_string(),
    });
    renderer.emit(&SystemEvent::InferenceCompleted {
        usage: Usage {
            response_chars: 2,
            tool_calls: 0,
            input_tokens: 80,
            output_tokens: 20,
            total_tokens: Some(100),
            cost: None,
            cost_complete: false,
        },
    });
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes).expect("utf8");
    assert!(stderr_out.contains("100 tokens"));
    assert!(stderr_out.contains("80 in + 20 out"));
}
