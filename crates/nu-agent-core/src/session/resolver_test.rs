use crate::protocol::contracts::UiMessageSnapshot;
use crate::session::{CompactionMarker, SessionStore as _, StoreEntry};
use crate::types::{
    AdditionalParams, AssistantContent, Message, Text, ToolCall, ToolCallId, ToolFunction,
    ToolResult, ToolResultContent, UserContent,
};
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;

/// Helper function to convert rig messages to UI snapshots for testing.
/// Delegates to the actual hydrate_single_message function in resolver.rs.
fn convert_rig_messages_to_snapshots(messages: &[Message]) -> Vec<UiMessageSnapshot> {
    let tool_names = std::collections::HashMap::new();
    let tool_success_map = std::collections::HashMap::new();
    messages
        .iter()
        .flat_map(|m| super::resolver::hydrate_single_message(m, &tool_names, &tool_success_map))
        .collect()
}

#[test]
fn test_convert_user_text() {
    let messages = vec![Message::User {
        content: vec![UserContent::Text(Text {
            text: "Hello, world!".to_string(),
            additional_params: None,
        })],
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
        content: vec![AssistantContent::Text(Text {
            text: "I can help you with that.".to_string(),
            additional_params: None,
        })],
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
        content: vec![AssistantContent::ToolCall(ToolCall {
            id: ToolCallId::new_or_mint("call_123"),
            provider: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: "read_file".to_string(),
                arguments: json!({"path": "/tmp/test.txt"}),
            },
        })],
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert!(snapshots[0].content().starts_with("→ "));
    assert_eq!(snapshots[0].tool_name(), Some("read_file"));
    assert!(snapshots[0].tool_arguments().is_some());
    assert_eq!(
        snapshots[0].tool_success(),
        None,
        "no persisted verdict flag for the call id — tool_success stays None"
    );
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
        content: vec![UserContent::ToolResult(ToolResult {
            call: ToolCallId::new_or_mint("call_123"),
            provider: None,
            name: "read_file".into(),
            content: vec![ToolResultContent::Text(Text {
                text: "File contents here".to_string(),
                additional_params: None,
            })],
        })],
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    // Tool results are kept in memory for the LLM but not shown in the TUI transcript
    assert_eq!(snapshots.len(), 0);
}

#[test]
fn test_convert_mixed_assistant_content() {
    let messages = vec![Message::Assistant {
        id: None,
        content: vec![
            AssistantContent::Text(Text {
                text: "Let me help you.".to_string(),
                additional_params: None,
            }),
            AssistantContent::ToolCall(ToolCall {
                id: ToolCallId::new_or_mint("call_weather"),
                provider: None,
                signature: None,
                additional_params: None,
                function: ToolFunction {
                    name: "get_weather".to_string(),
                    arguments: json!({"location": "NYC"}),
                },
            }),
        ],
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    // Should produce 2 snapshots: one for text, one for tool call
    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].role(), "assistant");
    assert_eq!(snapshots[0].content(), "Let me help you.");
    assert_eq!(snapshots[1].role(), "tool");
    assert!(snapshots[1].content().starts_with("→ "));
    assert_eq!(snapshots[1].tool_name(), Some("get_weather"));
}

#[test]
fn test_convert_multiple_messages() {
    let messages = vec![
        Message::User {
            content: vec![UserContent::Text(Text {
                text: "What's the weather?".to_string(),
                additional_params: None,
            })],
        },
        Message::Assistant {
            id: None,
            content: vec![AssistantContent::Text(Text {
                text: "I'll check that for you.".to_string(),
                additional_params: None,
            })],
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
        content: vec![AssistantContent::Text(Text {
            text: "".to_string(),
            additional_params: None,
        })],
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
        content: vec![AssistantContent::ToolCall(ToolCall {
            id: ToolCallId::new_or_mint("call_abc"),
            provider: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: "k8s__list_pods".to_string(),
                arguments: json!({"namespace": "prod"}),
            },
        })],
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");

    // Content should be formatted as "→ {summary}"
    assert!(snapshots[0].content().starts_with("→ "));
    assert_eq!(snapshots[0].tool_name(), Some("k8s__list_pods"));

    // Tool arguments should contain the raw JSON
    let args = snapshots[0].tool_arguments().unwrap();
    assert!(args.contains("namespace"));
    assert!(args.contains("prod"));

    // Tool success is None without a persisted verdict flag for the call id
    assert_eq!(snapshots[0].tool_success(), None);
}

#[test]
fn test_tool_call_argument_summarization() {
    // Test that long arguments are properly summarized
    let long_content = "x".repeat(150);
    let messages = vec![Message::Assistant {
        id: None,
        content: vec![AssistantContent::ToolCall(ToolCall {
            id: ToolCallId::new_or_mint("call_long"),
            provider: None,
            signature: None,
            additional_params: None,
            function: ToolFunction {
                name: "write_file".to_string(),
                arguments: json!({"content": long_content}),
            },
        })],
    }];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    assert_eq!(snapshots.len(), 1);

    // Content should be truncated with ellipsis
    let content = snapshots[0].content();
    assert!(content.starts_with("→ "));
    assert_eq!(snapshots[0].tool_name(), Some("write_file"));
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
            content: vec![UserContent::Text(Text {
                text: "Hello".to_string(),
                additional_params: None,
            })],
        }),
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: vec![AssistantContent::Text(Text {
                text: "Hi there".to_string(),
                additional_params: None,
            })],
        }),
    ];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

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
            content: vec![UserContent::Text(Text {
                text: "Hello".to_string(),
                additional_params: None,
            })],
        }),
        StoreEntry::Marker(CompactionMarker::new(
            "Summary of older messages".to_string(),
            Utc::now(),
        )),
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: vec![AssistantContent::Text(Text {
                text: "Response after compaction".to_string(),
                additional_params: None,
            })],
        }),
    ];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 3);
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[1].role(), "compaction");
    assert_eq!(snapshots[2].role(), "assistant");
}

#[test]
fn hydrate_store_entries_marker_format() {
    let entries = vec![StoreEntry::Marker(CompactionMarker::new(
        "The user asked about weather and got a response.".to_string(),
        Utc::now(),
    ))];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "compaction");

    let content = snapshots[0].content();
    // Content must include the summary body
    assert!(
        content.contains("The user asked about weather and got a response."),
        "Expected summary body in content, got: {content}"
    );
}

#[test]
fn hydrate_store_entries_preserves_order() {
    let entries = vec![
        StoreEntry::Message(Message::User {
            content: vec![UserContent::Text(Text {
                text: "First".to_string(),
                additional_params: None,
            })],
        }),
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: vec![AssistantContent::Text(Text {
                text: "Second".to_string(),
                additional_params: None,
            })],
        }),
        StoreEntry::Marker(CompactionMarker::new(
            "compaction summary".to_string(),
            Utc::now(),
        )),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::Text(Text {
                text: "Third".to_string(),
                additional_params: None,
            })],
        }),
    ];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

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
        Utc::now(),
    ))];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "compaction");

    let content = snapshots[0].content();
    // Empty summary produces empty content
    assert!(
        content.is_empty(),
        "Expected empty content for empty-summary marker, got: {content}"
    );
}

#[test]
fn test_tool_result_edit_creates_display_snapshot() {
    let entries = vec![
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall {
                id: ToolCallId::new_or_mint("call_1"),
                provider: None,
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
            })],
        }),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("call_1"),
                provider: None,
                name: "edit".into(),
                content: vec![ToolResultContent::Text(Text {
                    text: serde_json::to_string(&json!({
                        "path": "/tmp/test.rs",
                        "diff": "- old code\n+ new code",
                        "stats": { "insertions": 1, "deletions": 1 }
                    }))
                    .unwrap(),
                    additional_params: None,
                })],
            })],
        }),
    ];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].role(), "tool");

    let display = snapshots[1].tool_display();
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
            content: vec![AssistantContent::ToolCall(ToolCall {
                id: ToolCallId::new_or_mint("call_2"),
                provider: None,
                signature: None,
                additional_params: None,
                function: ToolFunction {
                    name: "read".to_string(),
                    arguments: json!({"path": "/tmp/test.txt"}),
                },
            })],
        }),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("call_2"),
                provider: None,
                name: "read".into(),
                content: vec![ToolResultContent::Text(Text {
                    text: "plain text, not JSON".to_string(),
                    additional_params: None,
                })],
            })],
        }),
    ];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    // Only tool invocation snapshot, no display for non-JSON result
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
}

#[test]
fn test_tool_result_with_explicit_display_key() {
    let entries = vec![
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall {
                id: ToolCallId::new_or_mint("call_3"),
                provider: None,
                signature: None,
                additional_params: None,
                function: ToolFunction {
                    name: "custom_tool".to_string(),
                    arguments: json!({}),
                },
            })],
        }),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("call_3"),
                provider: None,
                name: "custom_tool".into(),
                content: vec![ToolResultContent::Text(Text {
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
                })],
            })],
        }),
    ];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 2);

    let display = snapshots[1].tool_display();
    assert!(
        display.is_some(),
        "Expected tool_display on second snapshot"
    );
    let display = display.unwrap();
    assert_eq!(display.title, "custom output");
}

// --- Compaction marker with stats test (task spec) ---

/// Verifies that a Vec<StoreEntry> containing pre-compaction messages, a Marker, and
/// post-compaction messages produces a transcript where:
/// - The compaction entry appears at the correct position
/// - The compaction entry has role "compaction"
/// - The compaction entry content includes the summary body
#[test]
fn hydrate_store_entries_marker_shows_summary_body() {
    let entries = vec![
        StoreEntry::Message(Message::User {
            content: vec![UserContent::Text(Text {
                text: "pre-compaction message 1".to_string(),
                additional_params: None,
            })],
        }),
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: vec![AssistantContent::Text(Text {
                text: "pre-compaction reply".to_string(),
                additional_params: None,
            })],
        }),
        StoreEntry::Marker(CompactionMarker::new(
            "History summarized: user asked about Rust, assistant explained ownership.".to_string(),
            Utc::now(),
        )),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::Text(Text {
                text: "post-compaction question".to_string(),
                additional_params: None,
            })],
        }),
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: vec![AssistantContent::Text(Text {
                text: "post-compaction answer".to_string(),
                additional_params: None,
            })],
        }),
    ];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    // There are 5 entries: 2 pre-compaction messages, 1 marker, 2 post-compaction messages
    assert_eq!(
        snapshots.len(),
        5,
        "expected 5 snapshots, got: {snapshots:?}"
    );

    // Pre-compaction messages come first
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[0].content(), "pre-compaction message 1");
    assert_eq!(snapshots[1].role(), "assistant");
    assert_eq!(snapshots[1].content(), "pre-compaction reply");

    // Compaction marker is at index 2
    assert_eq!(
        snapshots[2].role(),
        "compaction",
        "expected compaction role at index 2"
    );
    let compaction_content = snapshots[2].content();

    // Summary body must be present
    assert!(
        compaction_content.contains("History summarized"),
        "expected summary body in compaction content, got: {compaction_content}"
    );

    // Post-compaction messages follow
    assert_eq!(snapshots[3].role(), "user");
    assert_eq!(snapshots[3].content(), "post-compaction question");
    assert_eq!(snapshots[4].role(), "assistant");
    assert_eq!(snapshots[4].content(), "post-compaction answer");
}

// --- tool_success rehydration tests ---

/// A row whose failure-shaped text ("Toolset error: ...") carries NO persisted
/// flag rehydrates as tool_success == None — output text is never consulted.
#[test]
fn hydrate_unflagged_toolset_error_text_rehydrates_as_none() {
    let entries = vec![
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall {
                id: ToolCallId::new_or_mint("call_fail_1"),
                provider: None,
                signature: None,
                additional_params: None,
                function: ToolFunction {
                    name: "bash".to_string(),
                    arguments: json!({"command": "exit 1"}),
                },
            })],
        }),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("call_fail_1"),
                provider: None,
                name: "bash".into(),
                content: vec![ToolResultContent::Text(Text {
                    text: "Toolset error: command exited with code 1".to_string(),
                    additional_params: None,
                })],
            })],
        }),
    ];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    // Should produce 1 snapshot: the tool call (tool result is skipped in TUI)
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        None,
        "unflagged Toolset error text must rehydrate as None — no text sniffing"
    );
}

/// A plain success-shaped row without a persisted flag rehydrates as
/// tool_success == None — output text is never consulted, no default.
#[test]
fn hydrate_unflagged_plain_text_rehydrates_as_none() {
    let entries = vec![
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall {
                id: ToolCallId::new_or_mint("call_ok_1"),
                provider: None,
                signature: None,
                additional_params: None,
                function: ToolFunction {
                    name: "read_file".to_string(),
                    arguments: json!({"path": "/tmp/test.txt"}),
                },
            })],
        }),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("call_ok_1"),
                provider: None,
                name: "read_file".into(),
                content: vec![ToolResultContent::Text(Text {
                    text: "file contents here".to_string(),
                    additional_params: None,
                })],
            })],
        }),
    ];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        None,
        "unflagged plain text must rehydrate as None — no default-to-success"
    );
}

/// Verifies that long summary text is shown in full (no truncation) in the compaction content.
#[test]
fn hydrate_store_entries_marker_shows_full_summary() {
    // Create a summary longer than the former 500-char truncation limit
    let long_summary = "x".repeat(600);
    let entries = vec![StoreEntry::Marker(CompactionMarker::new(
        long_summary.clone(),
        Utc::now(),
    ))];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 1);
    let content = snapshots[0].content();

    // Content must contain the full summary — no truncation occurred
    assert_eq!(
        content, long_summary,
        "expected full summary in content, got: {content}"
    );
}

/// A marker must hydrate without error and produce a "compaction" snapshot
/// whose body contains the marker summary text.
#[test]
fn hydrate_store_entries_marker_body_contains_summary() {
    let summary = "The user asked about weather and got a response about rain.";
    let entries = vec![StoreEntry::Marker(CompactionMarker::new(
        summary.to_string(),
        Utc::now(),
    ))];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "compaction");
    assert!(
        snapshots[0].content().contains(summary),
        "expected summary body in content, got: {}",
        snapshots[0].content()
    );
}

/// A row with legacy bare denial text ("Permission denied") and no persisted
/// flag rehydrates as tool_success == None — text is never consulted.
#[test]
fn hydrate_unflagged_permission_denied_text_rehydrates_as_none() {
    let entries = vec![
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall {
                id: ToolCallId::new_or_mint("call_perm_1"),
                provider: None,
                signature: None,
                additional_params: None,
                function: ToolFunction {
                    name: "write_file".to_string(),
                    arguments: json!({"path": "/etc/passwd", "content": "evil"}),
                },
            })],
        }),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("call_perm_1"),
                provider: None,
                name: "write_file".into(),
                content: vec![ToolResultContent::Text(Text {
                    text: "Permission denied".to_string(),
                    additional_params: None,
                })],
            })],
        }),
    ];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    // Should produce 1 snapshot: the tool call (tool result is skipped in TUI)
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        None,
        "unflagged permission denial text must rehydrate as None — no text sniffing"
    );
}

/// A row with legacy doom-loop text ("Doom loop detected: ...") and no
/// persisted flag rehydrates as tool_success == None — text is never consulted.
#[test]
fn hydrate_unflagged_doom_loop_text_rehydrates_as_none() {
    let entries = vec![
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall {
                id: ToolCallId::new_or_mint("call_doom_1"),
                provider: None,
                signature: None,
                additional_params: None,
                function: ToolFunction {
                    name: "nu".to_string(),
                    arguments: json!({"command": "ls"}),
                },
            })],
        }),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("call_doom_1"),
                provider: None,
                name: "nu".into(),
                content: vec![ToolResultContent::Text(Text {
                    text: "Doom loop detected: 'nu' called 5 times with identical arguments"
                        .to_string(),
                    additional_params: None,
                })],
            })],
        }),
    ];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    // Should produce 1 snapshot: the tool call (tool result is skipped in TUI)
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        None,
        "unflagged doom loop text must rehydrate as None — no text sniffing"
    );
}

// --- persisted success-verdict flag (nu_agent_success) ---

/// A persisted `nu_agent_success=true` verdict must win over output-text
/// sniffing: a successful tool whose output starts with `Tool '` rehydrates
/// as tool_success == Some(true). One snapshot per tool call is preserved.
#[test]
fn hydrate_flagged_tool_quote_text_rehydrates_as_true() {
    let entries = tool_call_entries(
        "call_flag_ok_1",
        "Tool 'nonexistent' is not available.",
        Some(true),
    );

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 1, "flag must not change snapshot count");
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        Some(true),
        "persisted nu_agent_success=true must win over the `Tool '` text sniff"
    );
}

/// The same `Tool '` text without a persisted flag rehydrates as
/// tool_success == None — the false-positive sniff class is dead.
#[test]
fn hydrate_unflagged_tool_quote_text_rehydrates_as_none() {
    let entries = tool_call_entries(
        "call_legacy_quote_1",
        "Tool 'nonexistent' is not available.",
        None,
    );

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        None,
        "unflagged `Tool '` text must rehydrate as None — no text sniffing"
    );
}

/// A genuine failure persisted with nu_agent_success=false rehydrates as
/// Some(false) — the flag, not the output text, decides.
#[test]
fn hydrate_flagged_failure_rehydrates_as_false() {
    let entries = tool_call_entries(
        "call_flag_fail_1",
        "read failed: No such file or directory (os error 2)",
        Some(false),
    );

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        Some(false),
        "persisted nu_agent_success=false must rehydrate as failure"
    );
}

/// A legacy row (no flag) with the enriched permission-denial text rehydrates
/// as tool_success == None — the flag is the only verdict source.
#[test]
fn hydrate_legacy_enriched_denial_rehydrates_as_none() {
    let entries = tool_call_entries(
        "call_legacy_denial_1",
        "Permission denied by rule 'global:*' (scope: global)",
        None,
    );

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        None,
        "legacy enriched denial text without a flag must rehydrate as None"
    );
}

/// A legacy row (no flag) whose text matches no failure class rehydrates as
/// tool_success == None — unknown is honest; no default-to-success guess.
#[test]
fn hydrate_legacy_unmatched_text_rehydrates_as_none() {
    let entries = tool_call_entries(
        "call_legacy_unmatched_1",
        "read failed: No such file or directory (os error 2)",
        None,
    );

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        None,
        "legacy rows with no flag must rehydrate as None"
    );
}

/// A first Text block whose `nu_agent_success` value is NOT a boolean (e.g. a
/// string) must not be trusted: the row rehydrates as tool_success == None.
#[test]
fn hydrate_non_boolean_flag_value_rehydrates_as_none() {
    let entries = vec![
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall {
                id: ToolCallId::new_or_mint("call_flag_bad_1"),
                provider: None,
                signature: None,
                additional_params: None,
                function: ToolFunction {
                    name: "read_file".to_string(),
                    arguments: json!({"path": "/tmp/test.txt"}),
                },
            })],
        }),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint("call_flag_bad_1"),
                provider: None,
                name: "read_file".into(),
                content: vec![ToolResultContent::Text(Text {
                    text: "file contents here".to_string(),
                    additional_params: AdditionalParams::from_entries([(
                        "nu_agent_success",
                        json!("yes"),
                    )]),
                })],
            })],
        }),
    ];

    let snapshots = super::resolver::hydrate_transcript_from_store_entries(&entries);

    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert_eq!(
        snapshots[0].tool_success(),
        None,
        "non-boolean nu_agent_success values must rehydrate as None"
    );
}

// -- Test Support

/// Build [assistant ToolCall, user ToolResult] store entries for one call
/// whose result text is `text` and whose persisted verdict flag is `flag`
/// (`None` = no flag — legacy row).
fn tool_call_entries(call_id: &str, text: &str, flag: Option<bool>) -> Vec<StoreEntry> {
    vec![
        StoreEntry::Message(Message::Assistant {
            id: None,
            content: vec![AssistantContent::ToolCall(ToolCall {
                id: ToolCallId::new_or_mint(call_id),
                provider: None,
                signature: None,
                additional_params: None,
                function: ToolFunction {
                    name: "read_file".to_string(),
                    arguments: json!({"path": "/tmp/test.txt"}),
                },
            })],
        }),
        StoreEntry::Message(Message::User {
            content: vec![UserContent::ToolResult(ToolResult {
                call: ToolCallId::new_or_mint(call_id),
                provider: None,
                name: "read_file".into(),
                content: vec![ToolResultContent::Text(Text {
                    text: text.to_string(),
                    additional_params: flag.and_then(|f| {
                        AdditionalParams::from_entries([("nu_agent_success", json!(f))])
                    }),
                })],
            })],
        }),
    ]
}

#[tokio::test]
async fn resolve_session_request_user_provided_id_gets_prefixed() {
    use crate::session::FsSessionStore;
    use crate::session::resolver::{
        DefaultSessionResolver, SessionResolutionInput, SessionResolver,
    };
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let resolver = DefaultSessionResolver::new(store);
    let cwd = std::path::PathBuf::from("/home/user/project");

    let result = resolver
        .resolve(SessionResolutionInput {
            use_tui: false,
            session_id: Some("foo".to_string()),
            cwd: cwd.clone(),
        })
        .await
        .unwrap();

    let id = result.final_session_id.unwrap();
    // Must start with 16-char prefix derived from cwd
    let prefix = crate::session::prefix::dir_prefix(&cwd);
    assert_eq!(&id[..17], format!("{prefix}-"));
    assert!(
        id.ends_with("foo"),
        "expected id to end with 'foo', got: {id}"
    );
}

#[tokio::test]
async fn resolve_auto_generated_id_gets_prefixed() {
    use crate::session::FsSessionStore;
    use crate::session::resolver::{
        DefaultSessionResolver, SessionResolutionInput, SessionResolver,
    };
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let resolver = DefaultSessionResolver::new(store);
    let cwd = std::path::PathBuf::from("/home/user/project");

    let result = resolver
        .resolve(SessionResolutionInput {
            use_tui: true,
            session_id: None,
            cwd: cwd.clone(),
        })
        .await
        .unwrap();

    let id = result.final_session_id.unwrap();
    let prefix = crate::session::prefix::dir_prefix(&cwd);
    assert!(
        id.starts_with(&format!("{prefix}-")),
        "expected id to start with '{prefix}-', got: {id}"
    );
}

#[tokio::test]
async fn attach_existing_session_always_returns_initial_messages() {
    use crate::session::FsSessionStore;
    use crate::session::resolver::{
        DefaultSessionResolver, SessionResolutionInput, SessionResolver,
    };
    use crate::types::Message;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let cwd = std::path::PathBuf::from("/home/user/project");

    // Pre-compute the prefixed session ID so we can write messages under it
    let prefix = crate::session::prefix::dir_prefix(&cwd);
    let raw_id = "existing-session";
    let prefixed_id = format!("{prefix}-{raw_id}");

    // Write messages to the store for that session ID
    let messages = vec![Message::user("hello"), Message::assistant("world")];
    store.create(&prefixed_id, &messages).await.unwrap();

    // Resolve with use_tui: true to trigger the Attach path for an explicit session_id.
    // The Attach branch now always loads existing store entries, with no input_is_nothing gate.
    let resolver = DefaultSessionResolver::new(Arc::clone(&store));
    let result = resolver
        .resolve(SessionResolutionInput {
            use_tui: true,
            session_id: Some(raw_id.to_string()),
            cwd: cwd.clone(),
        })
        .await
        .unwrap();

    assert!(
        result.should_hydrate_transcript,
        "should_hydrate_transcript must be true when session has messages"
    );
    assert!(
        !result.initial_messages.is_empty(),
        "initial_messages must be non-empty when session has messages"
    );
}

#[tokio::test]
async fn attach_existing_session_sets_last_total_tokens_none() {
    use crate::session::FsSessionStore;
    use crate::session::resolver::{
        DefaultSessionResolver, SessionResolutionInput, SessionResolver,
    };
    use crate::types::Message;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let cwd = std::path::PathBuf::from("/home/user/project");

    // Pre-compute the prefixed session ID so we can write messages under it
    let prefix = crate::session::prefix::dir_prefix(&cwd);
    let raw_id = "token-count-session";
    let prefixed_id = format!("{prefix}-{raw_id}");

    // Write messages to the store for that session ID
    let messages = vec![Message::user("hello"), Message::assistant("world")];
    store.create(&prefixed_id, &messages).await.unwrap();

    let resolver = DefaultSessionResolver::new(Arc::clone(&store));
    let result = resolver
        .resolve(SessionResolutionInput {
            use_tui: true,
            session_id: Some(raw_id.to_string()),
            cwd: cwd.clone(),
        })
        .await
        .unwrap();

    // Token estimation via the removed helpers is gone; last_total_tokens is None
    assert!(
        result.last_total_tokens.is_none(),
        "last_total_tokens must be None for existing sessions, got: {:?}",
        result.last_total_tokens
    );
}

#[tokio::test]
async fn attach_falls_back_to_legacy_prefixed_session() {
    use crate::session::FsSessionStore;
    use crate::session::resolver::{
        DefaultSessionResolver, SessionResolutionInput, SessionResolver,
    };
    use crate::types::Message;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let store = Arc::new(FsSessionStore::new(temp_dir.path().to_path_buf()));
    let cwd = std::path::PathBuf::from("/home/user/project");

    // Create a session with the legacy 7-char prefix (as if created before the
    // prefix length increase).
    let legacy_prefix = crate::session::prefix::dir_prefix_legacy(&cwd);
    let raw_id = "old-session";
    let legacy_id = format!("{legacy_prefix}-{raw_id}");
    store
        .create(&legacy_id, &[Message::user("hello")])
        .await
        .unwrap();

    let resolver = DefaultSessionResolver::new(Arc::clone(&store));
    let result = resolver
        .resolve(SessionResolutionInput {
            use_tui: true,
            session_id: Some(raw_id.to_string()),
            cwd: cwd.clone(),
        })
        .await
        .unwrap();

    // The resolved final_session_id must be the legacy-prefixed ID
    assert_eq!(
        result.final_session_id.as_deref(),
        Some(legacy_id.as_str()),
        "expected legacy-prefixed session to be resolved via fallback"
    );
    assert!(
        result.should_hydrate_transcript,
        "should_hydrate_transcript must be true when legacy session has messages"
    );
}
