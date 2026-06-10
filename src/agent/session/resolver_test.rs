use crate::agent::protocol::contracts::UiMessageSnapshot;
use crate::session::{CompactionMarker, StoreEntry};
use rig::completion::Message;
use rig::completion::message::{
    AssistantContent, Text, ToolCall, ToolFunction, ToolResultContent, UserContent,
};
use rig::one_or_many::OneOrMany;
use serde_json::json;

/// Helper function to convert rig messages to UI snapshots for testing.
/// Delegates to the actual hydrate_single_message function in resolver.rs.
fn convert_rig_messages_to_snapshots(messages: &[Message]) -> Vec<UiMessageSnapshot> {
    let tool_names = std::collections::HashMap::new();
    messages
        .iter()
        .flat_map(|m| super::hydrate_single_message(m, &tool_names))
        .collect()
}

#[test]
fn test_convert_user_text() {
    let messages = vec![Message::User {
        content: OneOrMany::one(UserContent::Text(Text {
            text: "Hello, world!".to_string(),
            additional_params: None,
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
            additional_params: None,
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
                    additional_params: None,
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
                additional_params: None,
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
                additional_params: None,
            })),
        },
        Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::Text(Text {
                text: "I'll check that for you.".to_string(),
                additional_params: None,
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
            additional_params: None,
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

// --- hydrate_transcript_from_store_entries tests ---

#[test]
fn hydrate_store_entries_includes_messages() {
    let entries = vec![
        StoreEntry::Message(Message::User {
            content: OneOrMany::one(UserContent::Text(Text {
                text: "Hello".to_string(),
                additional_params: None,
            })),
        }),
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::Text(Text {
                text: "Hi there".to_string(),
                additional_params: None,
            })),
        }),
    ];

    let snapshots = super::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[0].content(), "Hello");
    assert_eq!(snapshots[1].role(), "assistant");
    assert_eq!(snapshots[1].content(), "Hi there");
}

#[test]
fn hydrate_store_entries_includes_markers() {
    let entries = vec![
        StoreEntry::Message(Message::User {
            content: OneOrMany::one(UserContent::Text(Text {
                text: "Hello".to_string(),
                additional_params: None,
            })),
        }),
        StoreEntry::Marker(CompactionMarker::new(
            "Summary of older messages".to_string(),
            3,
            10,
            "SummarizeOldest",
        )),
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::Text(Text {
                text: "Response after compaction".to_string(),
                additional_params: None,
            })),
        }),
    ];

    let snapshots = super::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[1].role(), "compaction");
    assert_eq!(snapshots[2].role(), "assistant");
}

#[test]
fn hydrate_store_entries_marker_format() {
    let entries = vec![StoreEntry::Marker(CompactionMarker::new(
        "The user asked about weather and got a response.".to_string(),
        3,
        10,
        "SummarizeOldest",
    ))];

    let snapshots = super::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "compaction");

    let content = snapshots[0].content();
    assert_eq!(
        content, "The user asked about weather and got a response.",
        "Expected content to be the marker's summary text directly, got: {content}"
    );
}

#[test]
fn hydrate_store_entries_preserves_order() {
    let entries = vec![
        StoreEntry::Message(Message::User {
            content: OneOrMany::one(UserContent::Text(Text {
                text: "First".to_string(),
                additional_params: None,
            })),
        }),
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::Text(Text {
                text: "Second".to_string(),
                additional_params: None,
            })),
        }),
        StoreEntry::Marker(CompactionMarker::new(
            "compaction summary".to_string(),
            2,
            5,
            "SummarizeOldest",
        )),
        StoreEntry::Message(Message::User {
            content: OneOrMany::one(UserContent::Text(Text {
                text: "Third".to_string(),
                additional_params: None,
            })),
        }),
    ];

    let snapshots = super::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 4);
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[0].content(), "First");
    assert_eq!(snapshots[1].role(), "assistant");
    assert_eq!(snapshots[1].content(), "Second");
    assert_eq!(snapshots[2].role(), "compaction");
    assert_eq!(snapshots[3].role(), "user");
    assert_eq!(snapshots[3].content(), "Third");
}

#[test]
fn hydrate_store_entries_empty_summary_marker() {
    let entries = vec![StoreEntry::Marker(CompactionMarker::new(
        String::new(),
        5,
        8,
        "SlidingWindow",
    ))];

    let snapshots = super::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "compaction");

    let content = snapshots[0].content();
    assert_eq!(
        content, "",
        "Empty summary should render as empty string, got: {content}"
    );
}

#[test]
fn test_tool_result_edit_creates_display_snapshot() {
    let entries = vec![
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::ToolCall(ToolCall {
                id: "call_1".to_string(),
                call_id: None,
                signature: None,
                additional_params: None,
                function: ToolFunction {
                    name: "edit".to_string(),
                    arguments: json!({
                        "filePath": "/tmp/test.rs",
                        "oldString": "old code",
                        "newString": "new code"
                    }),
                },
            })),
        }),
        StoreEntry::Message(Message::User {
            content: OneOrMany::one(UserContent::ToolResult(
                rig::completion::message::ToolResult {
                    id: "call_1".to_string(),
                    call_id: None,
                    content: OneOrMany::one(ToolResultContent::Text(Text {
                        text: serde_json::to_string(&json!({
                            "path": "/tmp/test.rs",
                            "diff": "- old code\n+ new code",
                            "stats": { "insertions": 1, "deletions": 1 }
                        }))
                        .unwrap(),
                        additional_params: None,
                    })),
                },
            )),
        }),
    ];

    let snapshots = super::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].role(), "tool");

    let display = snapshots[1].tool_display_ref();
    assert!(
        display.is_some(),
        "Expected tool_display on second snapshot"
    );
    let display = display.unwrap();
    assert_eq!(display.title, "edit /tmp/test.rs");
    assert_eq!(display.sections.len(), 1);
    assert_eq!(display.sections[0].language, "diff");
}

#[test]
fn test_tool_result_non_json_gracefully_skipped() {
    let entries = vec![
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::ToolCall(ToolCall {
                id: "call_2".to_string(),
                call_id: None,
                signature: None,
                additional_params: None,
                function: ToolFunction {
                    name: "read".to_string(),
                    arguments: json!({"path": "/tmp/test.txt"}),
                },
            })),
        }),
        StoreEntry::Message(Message::User {
            content: OneOrMany::one(UserContent::ToolResult(
                rig::completion::message::ToolResult {
                    id: "call_2".to_string(),
                    call_id: None,
                    content: OneOrMany::one(ToolResultContent::Text(Text {
                        text: "plain text, not JSON".to_string(),
                        additional_params: None,
                    })),
                },
            )),
        }),
    ];

    let snapshots = super::hydrate_transcript_from_store_entries(&entries);

    // Only tool invocation snapshot, no display for non-JSON result
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
}

#[test]
fn test_tool_result_with_explicit_display_key() {
    let entries = vec![
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: OneOrMany::one(AssistantContent::ToolCall(ToolCall {
                id: "call_3".to_string(),
                call_id: None,
                signature: None,
                additional_params: None,
                function: ToolFunction {
                    name: "custom_tool".to_string(),
                    arguments: json!({}),
                },
            })),
        }),
        StoreEntry::Message(Message::User {
            content: OneOrMany::one(UserContent::ToolResult(
                rig::completion::message::ToolResult {
                    id: "call_3".to_string(),
                    call_id: None,
                    content: OneOrMany::one(ToolResultContent::Text(Text {
                        text: serde_json::to_string(&json!({
                            "display": {
                                "title": "custom output",
                                "sections": [{
                                    "label": "output",
                                    "language": "text",
                                    "content": "some result"
                                }]
                            }
                        }))
                        .unwrap(),
                        additional_params: None,
                    })),
                },
            )),
        }),
    ];

    let snapshots = super::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 2);

    let display = snapshots[1].tool_display_ref();
    assert!(
        display.is_some(),
        "Expected tool_display on second snapshot"
    );
    let display = display.unwrap();
    assert_eq!(display.title, "custom output");
}

// --- SessionResolution cumulative token propagation tests ---

#[test]
fn session_resolution_carries_cumulative_tokens_from_construction() {
    let resolution = super::SessionResolution {
        final_session_id: Some("test-session".to_string()),
        session: None,
        tui_should_hydrate_transcript: true,
        tui_initial_messages: vec![UiMessageSnapshot::new("user", "hello")],
        session_cumulative_tokens: Some(4200),
    };

    assert_eq!(resolution.session_cumulative_tokens, Some(4200));
}

#[test]
fn session_resolution_none_cumulative_for_non_hydrated_path() {
    let resolution = super::SessionResolution {
        final_session_id: Some("new-session".to_string()),
        session: None,
        tui_should_hydrate_transcript: false,
        tui_initial_messages: Vec::new(),
        session_cumulative_tokens: None,
    };

    assert_eq!(resolution.session_cumulative_tokens, None);
}
