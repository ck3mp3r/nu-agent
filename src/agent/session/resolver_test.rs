use crate::agent::protocol::contracts::UiMessageSnapshot;
use rig::completion::Message;
use rig::completion::message::{
    AssistantContent, Text, ToolCall, ToolFunction, ToolResultContent, UserContent,
};
use rig::one_or_many::OneOrMany;
use serde_json::json;

/// Helper function to convert rig messages to UI snapshots for testing.
/// This mirrors the private function in resolver.rs
fn convert_rig_messages_to_snapshots(messages: &[Message]) -> Vec<UiMessageSnapshot> {
    messages
        .iter()
        .flat_map(|msg| {
            let mut snapshots = Vec::new();

            match msg {
                Message::User { content } => {
                    for item in content.iter() {
                        match item {
                            UserContent::Text(text) => {
                                snapshots.push(UiMessageSnapshot::new("user", text.text.clone()));
                            }
                            UserContent::ToolResult(_) => {
                                // Tool results are kept in memory/JSONL for the LLM,
                                // but not shown in the hydrated TUI transcript.
                            }
                            _ => {}
                        }
                    }
                }
                Message::Assistant { content, .. } => {
                    for item in content.iter() {
                        match item {
                            AssistantContent::Text(text)
                                if !text.text.is_empty() => {
                                    snapshots.push(UiMessageSnapshot::new(
                                        "assistant",
                                        text.text.clone(),
                                    ));
                            }
                            AssistantContent::ToolCall(tool_call) => {
                                // Tool calls: hydrate as tool invocation with proper format
                                let args_json =
                                    serde_json::to_string(&tool_call.function.arguments)
                                        .unwrap_or_else(|_| "{}".to_string());

                                // Summarize arguments to match live rendering
                                let args_summary =
                                    crate::agent::protocol::tool_args::summarize_tool_arguments(
                                        &args_json,
                                    );

                                // Format content to match what start_tool_call produces
                                let display_content = format!(
                                    "tool[{}] args={}",
                                    tool_call.function.name, args_summary
                                );

                                snapshots.push(
                                    UiMessageSnapshot::new("tool", display_content)
                                        .with_tool_details(Some(args_json), None, Some(true)),
                                );
                            }
                            _ => {}
                        }
                    }
                }
                Message::System { content } => {
                    snapshots.push(UiMessageSnapshot::new("system", content.clone()));
                }
            }

            snapshots
        })
        .collect()
}

#[test]
fn test_convert_user_text() {
    let messages = vec![Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "Hello, world!".to_string(),
        })),
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[0].content(), "Hello, world!");
}

#[test]
fn test_convert_assistant_text() {
    let messages = vec![Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::Text(Text {
            text: "I can help you with that.".to_string(),
        })),
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "assistant");
    assert_eq!(snapshots[0].content(), "I can help you with that.");
}

#[test]
fn test_convert_assistant_tool_call() {
    let messages = vec![Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::ToolCall(ToolCall {
            id: "call_123".to_string(),
            call_id: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: "read_file".to_string(),
                arguments: json!({"path": "/tmp/test.txt"}),
            },
        })),
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert!(snapshots[0].content().starts_with("tool[read_file] args="));
    assert!(snapshots[0].tool_arguments().is_some());
    assert_eq!(snapshots[0].tool_success(), Some(true));
}

#[test]
fn test_convert_system() {
    let messages = vec![Message::System {
        content: "Context compacted: 10 messages summarized".to_string(),
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "system");
    assert_eq!(
        snapshots[0].content(),
        "Context compacted: 10 messages summarized"
    );
}

#[test]
fn test_tool_result_not_shown_in_hydrated_transcript() {
    let messages = vec![Message::User {
        content: OneOrMany::one(UserContent::ToolResult(
            rig::completion::message::ToolResult {
                id: "call_123".to_string(),
                call_id: None,
                content: OneOrMany::one(ToolResultContent::Text(Text {
                    text: "File contents here".to_string(),
                })),
            },
        )),
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    // Tool results are kept in memory for the LLM but not shown in the TUI transcript
    assert_eq!(snapshots.len(), 0);
}

#[test]
fn test_convert_mixed_assistant_content() {
    let messages = vec![Message::Assistant {
        id: None,
        content: OneOrMany::many(vec![
            AssistantContent::Text(Text {
                text: "Let me help you.".to_string(),
            }),
            AssistantContent::ToolCall(ToolCall {
                id: "call_weather".to_string(),
                call_id: None,
                signature: None,
                additional_params: None,
                function: ToolFunction {
                    name: "get_weather".to_string(),
                    arguments: json!({"location": "NYC"}),
                },
            }),
        ])
        .unwrap(),
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    // Should produce 2 snapshots: one for text, one for tool call
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].role(), "assistant");
    assert_eq!(snapshots[0].content(), "Let me help you.");
    assert_eq!(snapshots[1].role(), "tool");
    assert!(
        snapshots[1]
            .content()
            .starts_with("tool[get_weather] args=")
    );
}

#[test]
fn test_convert_multiple_messages() {
    let messages = vec![
        Message::User {
            content: OneOrMany::one(UserContent::Text(Text {
                text: "What's the weather?".to_string(),
            })),
        },
        Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::Text(Text {
                text: "I'll check that for you.".to_string(),
            })),
        },
        Message::System {
            content: "Compaction summary".to_string(),
        },
    ];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[1].role(), "assistant");
    assert_eq!(snapshots[2].role(), "system");
}

#[test]
fn test_empty_assistant_text_skipped() {
    let messages = vec![Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::Text(Text {
            text: "".to_string(),
        })),
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    // Empty text should be skipped
    assert_eq!(snapshots.len(), 0);
}

#[test]
fn test_tool_call_format_matches_live_rendering() {
    // Test that tool call format matches what start_tool_call produces
    let messages = vec![Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::ToolCall(ToolCall {
            id: "call_abc".to_string(),
            call_id: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: "k8s__list_pods".to_string(),
                arguments: json!({"namespace": "prod"}),
            },
        })),
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");

    // Content should be formatted as "tool[name] args={summary}"
    assert!(
        snapshots[0]
            .content()
            .starts_with("tool[k8s__list_pods] args=")
    );

    // Tool arguments should contain the raw JSON
    let args = snapshots[0].tool_arguments().unwrap();
    assert!(args.contains("namespace"));
    assert!(args.contains("prod"));

    // Tool success should be set to true for reloaded sessions
    assert_eq!(snapshots[0].tool_success(), Some(true));
}

#[test]
fn test_tool_call_argument_summarization() {
    // Test that long arguments are properly summarized
    let long_content = "x".repeat(150);
    let messages = vec![Message::Assistant {
        id: None,
        content: OneOrMany::one(AssistantContent::ToolCall(ToolCall {
            id: "call_long".to_string(),
            call_id: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: "write_file".to_string(),
                arguments: json!({"content": long_content}),
            },
        })),
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    assert_eq!(snapshots.len(), 1);

    // Content should be truncated with ellipsis
    let content = snapshots[0].content();
    assert!(content.starts_with("tool[write_file] args="));
    assert!(content.len() < 200); // Should be less than content length + overhead

    // But raw arguments should contain full JSON
    let args = snapshots[0].tool_arguments().unwrap();
    assert!(args.contains(&long_content));
}
