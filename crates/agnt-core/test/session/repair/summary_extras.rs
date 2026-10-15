use super::*;

// ================================================================
// CompactingMemory artifact shape
// ================================================================

/// `trim_trailing_user` only pops from the end, so a leading summary artifact
/// survives; `merge_consecutive_same_role` never folds the summary artifact
/// into the next user message.
#[test]
fn summary_artifact_survives_trim_and_merge() {
    // -- Case 1: a leading artifact survives the trailing-user trim
    let msgs = vec![
        summary_artifact("compacted summary"),
        Message::assistant("kept reply"),
        Message::user("trailing turn"),
    ];
    let mut issues = Vec::new();
    let result = trim_trailing_user(msgs, &mut issues);

    assert_eq!(result.len(), 2, "only the trailing user is trimmed");
    match &result[0] {
        Message::User { content } => assert!(
            is_summary_content(content),
            "leading artifact must survive trim_trailing_user"
        ),
        _ => panic!("expected user message"),
    }
    assert!(
        !issues.is_empty(),
        "trailing user trim must emit a diagnostic"
    );

    // -- Case 2: the artifact is not merged with the next user message
    let msgs = vec![
        summary_artifact("compacted summary"),
        Message::user("kept turn"),
        Message::assistant("reply"),
    ];
    let mut issues = Vec::new();
    let result = merge_consecutive_same_role(msgs, &mut issues);

    assert_eq!(
        result.len(),
        3,
        "summary artifact must NOT be merged with the next user message"
    );
    match &result[0] {
        Message::User { content } => {
            assert_eq!(content.len(), 1, "summary must remain its own message");
            assert!(
                is_summary_content(content),
                "first message must still be the summary artifact"
            );
        }
        _ => panic!("expected user message"),
    }
    assert!(
        issues.is_empty(),
        "no merge should be recorded for the summary artifact"
    );
}

/// Plain-text user messages are still merged, and `fix_tool_call_integrity`
/// still removes dangling ToolCalls after the artifact shape change.
#[test]
fn merge_and_integrity_behaviour_is_preserved() {
    // -- Case 1: consecutive plain-text users are merged
    let msgs = vec![
        Message::user("first"),
        Message::user("second"),
        Message::assistant("reply"),
    ];
    let mut issues = Vec::new();
    let result = merge_consecutive_same_role(msgs, &mut issues);

    assert_eq!(
        result.len(),
        2,
        "two text-only user messages must be merged"
    );
    match &result[0] {
        Message::User { content } => assert_eq!(content.len(), 2),
        _ => panic!("expected user message"),
    }
    assert!(!issues.is_empty(), "merge must emit a diagnostic");

    // -- Case 2: a dangling ToolCall is still removed
    let msgs = vec![
        summary_artifact("compacted summary"),
        Message::user("kept turn"),
        assistant_with_content(vec![make_tool_call("X", "do_thing")]),
    ];
    let mut issues = Vec::new();
    let result = fix_tool_call_integrity(msgs, &mut issues);

    let has_tool_call = result.iter().any(|msg| match msg {
        Message::Assistant(assistant) => assistant
            .content
            .iter()
            .any(|i| matches!(i, AssistantContent::ToolCall(_))),
        _ => false,
    });
    assert!(
        !has_tool_call,
        "dangling ToolCall must be removed by fix_tool_call_integrity"
    );
    assert!(!issues.is_empty(), "integrity pass must emit a diagnostic");
}
