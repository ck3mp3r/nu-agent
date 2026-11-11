//! Tool display and token-usage lines.

use super::*;

use agnt_runtime::session::tool_status::ToolStatus;

#[test]
fn tool_display_renders_diff_with_stats() -> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );
    // The result the `edit` handler returns: the diff it made and its
    // counters. The renderer derives every row from it.
    let result = concat!(
        "{\"path\":\"test.rs\",\"diff\":\"+added\\n-removed\\nunchanged\",",
        "\"stats\":{\"files_changed\":2,\"insertions\":3,\"deletions\":1}}"
    );
    draw(
        &mut renderer,
        SessionRecord::Tool(ToolRecord::Completed {
            call: ToolCall::new(
                "edit",
                agnt_runtime::tools::handler::ToolSource::Unknown,
                r#"{"path":"test.rs"}"#,
            ),
            outcome: ToolOutcome {
                status: ToolStatus::Success,
                payload: None,
                text: result.to_string(),
                message: None,
                kind: None,
            },
        }),
    );
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(stderr_out.contains("2 files changed"));
    Ok(())
}

#[test]
fn llm_end_prints_token_usage() -> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        false,
    );
    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Started),
    );
    narrate(
        &mut renderer,
        NarrationEvent::InferenceChunk {
            delta: "hi".to_string(),
            aggregated: "hi".to_string(),
        },
    );
    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Completed {
            usage: Usage {
                input_tokens: 80,
                output_tokens: 20,
                total_tokens: Some(100),
                cost: None,
                cost_complete: false,
            },
        }),
    );
    renderer.flush();
    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(stderr_out.contains("100 tokens"));
    assert!(stderr_out.contains("80 in + 20 out"));
    Ok(())
}
