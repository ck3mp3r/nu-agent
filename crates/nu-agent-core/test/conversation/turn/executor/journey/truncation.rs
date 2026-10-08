use super::*;

// ---------------------------------------------------------------------------
// Gap 2B: configurable tool result truncation limit
// ---------------------------------------------------------------------------

/// Verify that a tiny `max_tool_result_bytes` causes tool results to be
/// truncated when the response exceeds the limit.
#[tokio::test]
async fn journey_tool_result_truncated_at_configured_limit() -> Result<()> {
    use rig::message::{Message, UserContent};

    // Use a tiny limit to avoid large allocations in tests.
    let mut h = JourneyHarness::new_with_config(
        "journey-truncate",
        crate::config::Config {
            max_tool_result_bytes: Some(100),
            ..crate::config::Config::default()
        },
    );
    let (server, client) = h.start_mock_server().await?;

    // Mount responses: tool call first, then a plain text response
    {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("content-type", "text/event-stream")
                    .set_body_bytes(
                        sse_tool_call_response("tc1", "nu__shell", "{\"command\":\"ls\"}")
                            .into_bytes(),
                    ),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("content-type", "text/event-stream")
                    .set_body_bytes(sse_text_response("done").into_bytes()),
            )
            .mount(&server)
            .await;
    }

    // Tool returns 200 bytes — well over the 100-byte limit.
    let response_200: &'static str = Box::leak("x".repeat(200).into_boxed_str());
    let (r, _) = h
        .turn_with_client(
            "list files",
            &client,
            nu_shell_tool_truncating(response_200, 100).await,
        )
        .await?;
    assert!(r.is_ok(), "turn must succeed: {r:?}");

    let msgs = h.raw_messages().await?;
    // Expect [user(prompt), asst(tool_call), user(tool_result), asst(final)]
    assert!(
        msgs.len() >= 3,
        "expected at least 3 messages, got: {}",
        msgs.len()
    );

    // The tool result is in the third message (index 2), which is a user message
    // containing a ToolResult with the truncated output.
    let tool_result_text = msgs
        .iter()
        .find_map(|msg| {
            if let Message::User { content } = msg {
                content.iter().find_map(|c| {
                    if let UserContent::ToolResult(tr) = c {
                        use crate::types::ToolResultContent;
                        let text = tr
                            .content
                            .iter()
                            .map(|tc| match tc {
                                ToolResultContent::Text(t) => t.text.clone(),
                                ToolResultContent::Image(_) => String::new(),
                                ToolResultContent::Json { value } => value.to_string(),
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        Some(text)
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
        .ok_or("expected a ToolResult message")?;

    assert!(
        tool_result_text.contains("[output truncated"),
        "tool result must be truncated at 100-byte limit, got: {tool_result_text:?}"
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Gap 2B (wiring): Config::max_tool_result_bytes → BuiltinToolAdapter
// ---------------------------------------------------------------------------

/// Register a `grep` tool backed by a real `make_dynamic_tool::<GrepTool>` using the
/// provided `max_tool_result_bytes` limit and temp directory (caller creates the file
/// content inside `cwd`).  This is the only builtin-tool path we can exercise in
/// `nu-agent-core` tests.
async fn grep_via_builtin_adapter(
    max_tool_result_bytes: usize,
    cwd: std::path::PathBuf,
) -> ToolInfra {
    use crate::tools::handler::builtin_tool::make_dynamic_tool;
    use crate::tools::handler::grep::GrepTool;
    use crate::types::ToolDefinition;

    let tool_def = ToolDefinition {
        name: "grep".to_string(),
        description: "Search file contents".to_string(),
        parameters: serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" }
            },
            "required": ["pattern"]
        }),
    };
    let bus = crate::bus::Bus::default();
    let handle = rig::tool::server::ToolServer::new().run();
    handle.add_dynamic_tool(make_dynamic_tool::<GrepTool>(
        tool_def,
        cwd.clone(),
        max_tool_result_bytes,
        bus,
    ));
    default_tool_infra(
        handle,
        vec![rig::completion::ToolDefinition {
            name: "grep".to_string(),
            description: "Search file contents".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string" }
                },
                "required": ["pattern"]
            }),
        }],
    )
}

/// Prove `Config::max_tool_result_bytes → BuiltinToolAdapter::max_tool_result_bytes →
/// truncate_tool_output` wiring is correct end-to-end.
///
/// Unlike `journey_tool_result_truncated_at_configured_limit` (which exercises a
/// hand-rolled `TestTruncatingNuShellTool`), this test registers a real
/// `BuiltinToolAdapter` (via `skill_via_builtin_adapter`) and verifies that the limit
/// from the harness config is respected when the adapter serialises and caps the result.
#[tokio::test]
async fn journey_tool_result_limit_flows_from_config_to_adapter() -> Result<()> {
    use rig::message::{Message, UserContent};

    // ── 1. Harness with a 100-byte limit ─────────────────────────────────────
    let limit = 100usize;
    let mut h = JourneyHarness::new_with_config(
        "journey-builtin-adapter-truncate",
        crate::config::Config {
            max_tool_result_bytes: Some(limit),
            ..crate::config::Config::default()
        },
    );
    let (server, client) = h.start_mock_server().await?;

    // ── 2. Create a file whose serialised grep JSON will exceed 100 bytes ───
    //    The content has long matching lines; once wrapped in JSON
    //    (`{"matches":[…],"total":…}`) the total is well over 100 bytes.
    let temp_dir = tempfile::tempdir().expect("tempdir");
    let long_line = format!("needle {}\n", "x".repeat(200));
    std::fs::write(temp_dir.path().join("big.txt"), long_line.repeat(10)).expect("write file");

    // ── 3. Mount: tool call → text response ──────────────────────────────────
    {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, ResponseTemplate};

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("content-type", "text/event-stream")
                    .set_body_bytes(
                        sse_tool_call_response("tc-adapter", "grep", "{\"pattern\":\"needle\"}")
                            .into_bytes(),
                    ),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .append_header("content-type", "text/event-stream")
                    .set_body_bytes(sse_text_response("done").into_bytes()),
            )
            .mount(&server)
            .await;
    }

    // ── 4. Run the turn with the BuiltinToolAdapter-backed ToolInfra ─────────
    let cwd = temp_dir.path().to_path_buf();
    let (r, _) = h
        .turn_with_client(
            "load skill",
            &client,
            grep_via_builtin_adapter(limit, cwd).await,
        )
        .await?;
    assert!(r.is_ok(), "turn must succeed: {r:?}");

    // ── 5. Assert the tool result was truncated ───────────────────────────────
    let msgs = h.raw_messages().await?;
    assert!(
        msgs.len() >= 3,
        "expected at least 3 messages, got: {}",
        msgs.len()
    );

    let tool_result_text = msgs
        .iter()
        .find_map(|msg| {
            if let Message::User { content } = msg {
                content.iter().find_map(|c| {
                    if let UserContent::ToolResult(tr) = c {
                        use crate::types::ToolResultContent;
                        let text = tr
                            .content
                            .iter()
                            .map(|tc| match tc {
                                ToolResultContent::Text(t) => t.text.clone(),
                                ToolResultContent::Image(_) => String::new(),
                                ToolResultContent::Json { value } => value.to_string(),
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        Some(text)
                    } else {
                        None
                    }
                })
            } else {
                None
            }
        })
        .ok_or("expected a ToolResult message")?;

    assert!(
        tool_result_text.contains("[output truncated"),
        "BuiltinToolAdapter must truncate at {limit}-byte limit; got: {tool_result_text:?}"
    );
    Ok(())
}
