use super::*;

#[test]
fn default_tool_lifecycle_is_single_completion_line_with_result_block()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    let mut stderr_bytes = Vec::<u8>::new();
    let mut renderer = StderrUiRenderer::new(
        &mut stderr_bytes,
        UiPolicy {
            quiet: false,
            verbosity: Verbosity::Normal,
        },
        true,
    );

    draw(
        &mut renderer,
        SessionRecord::Inference(InferenceRecord::Started {
            inference: InferenceId::mint(),
        }),
    );
    draw(
        &mut renderer,
        SessionRecord::Tool(ToolRecord::Started {
            call: ToolCall::new(
                "gh__list_prs",
                agnt_runtime::tools::handler::ToolSource::Mcp,
                "{}",
            ),
        }),
    );
    draw(
        &mut renderer,
        SessionRecord::Tool(ToolRecord::Completed {
            call: ToolCall::new(
                "gh__list_prs",
                agnt_runtime::tools::handler::ToolSource::Mcp,
                "{}",
            ),
            outcome: ToolOutcome {
                status: ToolStatus::Success,
                payload: None,
                text: "[]".to_string(),
                message: None,
                kind: None,
            },
        }),
    );
    renderer.flush();

    let stderr_out = String::from_utf8(stderr_bytes)?;
    assert!(stderr_out.contains("✓ tool gh__list_prs → {}"));
    assert!(stderr_out.contains("\n[]"));
    assert!(!stderr_out.contains("→ tool gh__list_prs"));
    Ok(())
}

#[test]
fn default_tool_lifecycle_prints_non_empty_payloads()
-> core::result::Result<(), Box<dyn std::error::Error>> {
    for payload in ["[]", "{}", "null", ""] {
        let mut stderr_bytes = Vec::<u8>::new();
        let mut renderer = StderrUiRenderer::new(
            &mut stderr_bytes,
            UiPolicy {
                quiet: false,
                verbosity: Verbosity::Normal,
            },
            true,
        );

        draw(
            &mut renderer,
            SessionRecord::Tool(ToolRecord::Started {
                call: ToolCall::new(
                    "gh__list_prs",
                    agnt_runtime::tools::handler::ToolSource::Mcp,
                    "{}",
                ),
            }),
        );
        draw(
            &mut renderer,
            SessionRecord::Tool(ToolRecord::Completed {
                call: ToolCall::new(
                    "gh__list_prs",
                    agnt_runtime::tools::handler::ToolSource::Mcp,
                    "{}",
                ),
                outcome: ToolOutcome {
                    status: ToolStatus::Success,
                    payload: None,
                    text: payload.to_string(),
                    message: None,
                    kind: None,
                },
            }),
        );
        renderer.flush();

        let stderr_out = String::from_utf8(stderr_bytes)?;
        assert!(stderr_out.contains("✓ tool gh__list_prs → {}"));
        if payload.is_empty() {
            assert!(!stderr_out.contains("\n[]"));
            assert!(!stderr_out.contains("\n{}"));
            assert!(!stderr_out.contains("\nnull"));
        } else {
            assert!(stderr_out.contains(&format!("\n{payload}")));
        }
    }
    Ok(())
}
