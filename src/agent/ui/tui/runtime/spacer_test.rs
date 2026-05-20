use super::*;
use crate::agent::ui::transcript::ir::{ContentLine, StyleHint};
use crate::agent::ui::transcript::items::*;

#[test]
fn insert_spacers_empty() {
    let entries = vec![];
    let result = insert_spacers(entries);
    assert_eq!(result.len(), 0);
}

#[test]
fn insert_spacers_single_entry() {
    let entries = vec![TranscriptEntry::User(UserMessage {
        text: "hi".to_string(),
    })];
    let result = insert_spacers(entries);
    assert_eq!(result.len(), 1);
    assert!(matches!(result[0], TranscriptEntry::User(_)));
}

#[test]
fn insert_spacers_same_role_no_spacer() {
    let entries = vec![
        TranscriptEntry::Assistant(AssistantChunk {
            lines: vec![ContentLine::single("first".to_string(), StyleHint::Normal)],
        }),
        TranscriptEntry::Assistant(AssistantChunk {
            lines: vec![ContentLine::single("second".to_string(), StyleHint::Normal)],
        }),
    ];
    let result = insert_spacers(entries);
    assert_eq!(result.len(), 2);
    assert!(matches!(result[0], TranscriptEntry::Assistant(_)));
    assert!(matches!(result[1], TranscriptEntry::Assistant(_)));
}

#[test]
fn insert_spacers_user_then_tool() {
    let entries = vec![
        TranscriptEntry::User(UserMessage {
            text: "hi".to_string(),
        }),
        TranscriptEntry::Tool(ToolInvocation {
            name: "read".to_string(),
            source: "test".to_string(),
            args: "{}".to_string(),
        }),
    ];
    let result = insert_spacers(entries);
    assert_eq!(result.len(), 3);
    assert!(matches!(result[0], TranscriptEntry::User(_)));
    assert!(matches!(result[1], TranscriptEntry::Spacer(_)));
    assert!(matches!(result[2], TranscriptEntry::Tool(_)));
}

#[test]
fn insert_spacers_user_then_assistant() {
    let entries = vec![
        TranscriptEntry::User(UserMessage {
            text: "hi".to_string(),
        }),
        TranscriptEntry::Assistant(AssistantChunk {
            lines: vec![ContentLine::single("hello".to_string(), StyleHint::Normal)],
        }),
    ];
    let result = insert_spacers(entries);
    assert_eq!(result.len(), 2);
    assert!(matches!(result[0], TranscriptEntry::User(_)));
    assert!(matches!(result[1], TranscriptEntry::Assistant(_)));
}

#[test]
fn insert_spacers_tool_then_tool_display() {
    let entries = vec![
        TranscriptEntry::Tool(ToolInvocation {
            name: "read".to_string(),
            source: "test".to_string(),
            args: "{}".to_string(),
        }),
        TranscriptEntry::ToolResult(ToolResult {
            name: "read".to_string(),
            success: true,
            lines: vec![],
        }),
    ];
    let result = insert_spacers(entries);
    assert_eq!(result.len(), 2);
    assert!(matches!(result[0], TranscriptEntry::Tool(_)));
    assert!(matches!(result[1], TranscriptEntry::ToolResult(_)));
}

#[test]
fn insert_spacers_assistant_then_system() {
    let entries = vec![
        TranscriptEntry::Assistant(AssistantChunk {
            lines: vec![ContentLine::single("done".to_string(), StyleHint::Normal)],
        }),
        TranscriptEntry::System(SystemMessage {
            text: "system".to_string(),
        }),
    ];
    let result = insert_spacers(entries);
    assert_eq!(result.len(), 3);
    assert!(matches!(result[0], TranscriptEntry::Assistant(_)));
    assert!(matches!(result[1], TranscriptEntry::Spacer(_)));
    assert!(matches!(result[2], TranscriptEntry::System(_)));
}

#[test]
fn needs_spacer_no_previous() {
    assert!(!needs_spacer(None, &Role::User));
}

#[test]
fn needs_spacer_same_role() {
    assert!(!needs_spacer(Some(&Role::User), &Role::User));
}

#[test]
fn needs_spacer_separator_previous() {
    assert!(!needs_spacer(Some(&Role::Separator), &Role::User));
}

#[test]
fn needs_spacer_separator_next() {
    assert!(!needs_spacer(Some(&Role::User), &Role::Separator));
}

#[test]
fn needs_spacer_user_to_assistant() {
    assert!(!needs_spacer(Some(&Role::User), &Role::Assistant));
}

#[test]
fn needs_spacer_assistant_to_user() {
    assert!(!needs_spacer(Some(&Role::Assistant), &Role::User));
}

#[test]
fn needs_spacer_tool_to_tool_display() {
    assert!(!needs_spacer(Some(&Role::Tool), &Role::ToolDisplay));
}

#[test]
fn needs_spacer_tool_display_to_tool() {
    assert!(!needs_spacer(Some(&Role::ToolDisplay), &Role::Tool));
}

#[test]
fn needs_spacer_user_to_tool() {
    assert!(needs_spacer(Some(&Role::User), &Role::Tool));
}

#[test]
fn needs_spacer_assistant_to_system() {
    assert!(needs_spacer(Some(&Role::Assistant), &Role::System));
}
