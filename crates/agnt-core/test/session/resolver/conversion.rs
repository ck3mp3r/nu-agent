use super::*;

/// Plain user, assistant, and system messages convert to one snapshot each,
/// preserving order.
#[test]
fn test_convert_simple_messages() {
    // -- Case 1: user text
    let messages = vec![Message::User {
        content: vec![UserContent::Text(Text::new("Hello, world!"))],
    }];
    let snapshots = convert_rig_messages_to_snapshots(&messages);
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "user");
    assert_eq!(snapshots[0].content(), "Hello, world!");

    // -- Case 2: assistant text
    let messages = vec![Message::Assistant(AssistantMessage::new(vec![
        AssistantContent::Text(Text::new("I can help you with that.")),
    ]))];
    let snapshots = convert_rig_messages_to_snapshots(&messages);
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "assistant");
    assert_eq!(snapshots[0].content(), "I can help you with that.");

    // -- Case 3: system message
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

    // -- Case 4: multiple messages keep their order
    let messages = vec![
        Message::User {
            content: vec![UserContent::Text(Text::new("What's the weather?"))],
        },
        Message::Assistant(AssistantMessage::new(vec![AssistantContent::Text(
            Text::new("I'll check that for you."),
        )])),
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

/// A tool call converts to a `tool` snapshot whose content matches the live
/// `→ {summary}` rendering and whose raw arguments keep the full JSON.
#[test]
fn test_convert_tool_call() {
    // -- Case 1: a basic tool call
    let messages = vec![Message::Assistant(AssistantMessage::new(vec![
        AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire("call_123"),
            ToolFunction::new(
                ToolName::new("read_file").expect("should be non-empty"),
                json!({"path": "/tmp/test.txt"}),
            ),
        )),
    ]))];
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

    // -- Case 2: the format matches live rendering and args carry the raw JSON
    let messages = vec![Message::Assistant(AssistantMessage::new(vec![
        AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire("call_abc"),
            ToolFunction::new(
                ToolName::new("k8s__list_pods").expect("should be non-empty"),
                json!({"namespace": "prod"}),
            ),
        )),
    ]))];
    let snapshots = convert_rig_messages_to_snapshots(&messages);
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].role(), "tool");
    assert!(snapshots[0].content().starts_with("→ "));
    assert_eq!(snapshots[0].tool_name(), Some("k8s__list_pods"));
    let args = snapshots[0].tool_arguments().unwrap();
    assert!(args.contains("namespace"));
    assert!(args.contains("prod"));
    assert_eq!(snapshots[0].tool_success(), None);

    // -- Case 3: long arguments are summarized in content but kept raw in args
    let long_content = "x".repeat(150);
    let messages = vec![Message::Assistant(AssistantMessage::new(vec![
        AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire("call_long"),
            ToolFunction::new(
                ToolName::new("write_file").expect("should be non-empty"),
                json!({"content": long_content}),
            ),
        )),
    ]))];
    let snapshots = convert_rig_messages_to_snapshots(&messages);
    assert_eq!(snapshots.len(), 1);
    let content = snapshots[0].content();
    assert!(content.starts_with("→ "));
    assert_eq!(snapshots[0].tool_name(), Some("write_file"));
    assert!(content.len() < 200, "summary must be truncated");
    let args = snapshots[0].tool_arguments().unwrap();
    assert!(args.contains(&long_content));
}

/// Mixed assistant content produces one snapshot per content block.
#[test]
fn test_convert_mixed_assistant_content() {
    let messages = vec![Message::Assistant(AssistantMessage::new(vec![
        AssistantContent::Text(Text::new("Let me help you.")),
        AssistantContent::ToolCall(ToolCall::new(
            CallId::from_wire("call_weather"),
            ToolFunction::new(
                ToolName::new("get_weather").expect("should be non-empty"),
                json!({"location": "NYC"}),
            ),
        )),
    ]))];

    let snapshots = convert_rig_messages_to_snapshots(&messages);

    assert_eq!(snapshots.len(), 2);
    assert_eq!(snapshots[0].role(), "assistant");
    assert_eq!(snapshots[0].content(), "Let me help you.");
    assert_eq!(snapshots[1].role(), "tool");
    assert!(snapshots[1].content().starts_with("→ "));
    assert_eq!(snapshots[1].tool_name(), Some("get_weather"));
}

/// Tool results and empty assistant text produce no snapshot.
#[test]
fn test_convert_skips_tool_results_and_empty_text() {
    // -- Case 1: tool results are kept in memory but not shown in the transcript
    let messages = vec![Message::User {
        content: vec![UserContent::ToolResult(ToolResult {
            call: CallId::from_wire("call_123"),
            name: ToolName::new("read_file").expect("non-empty tool name"),
            content: vec![ToolResultContent::Text(Text::new("File contents here"))],
            is_error: false,
        })],
    }];
    assert_eq!(convert_rig_messages_to_snapshots(&messages).len(), 0);

    // -- Case 2: empty assistant text is skipped
    let messages = vec![Message::Assistant(AssistantMessage::new(vec![
        AssistantContent::Text(Text::new("")),
    ]))];
    assert_eq!(convert_rig_messages_to_snapshots(&messages).len(), 0);
}
