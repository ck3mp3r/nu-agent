use super::*;

/// `validate_tool_call_adjacency` keeps valid adjacent ToolCall/ToolResult
/// pairs and strips non-adjacent pairs.
#[test]
fn validate_tool_call_adjacency_keeps_adjacent_pairs() -> TestResult<()> {
    // -- Setup & Fixtures
    let valid_pair = vec![
        crate::types::Message::user("hi"),
        make_tool_call_msg("tc1")?,
        make_tool_result_msg("tc1")?,
    ];
    let multiple_pairs = vec![
        crate::types::Message::user("go"),
        make_tool_call_msg("tc1")?,
        make_tool_result_msg("tc1")?,
        make_tool_call_msg("tc2")?,
        make_tool_result_msg("tc2")?,
        crate::types::Message::assistant("done"),
    ];
    let non_adjacent = vec![
        crate::types::Message::user("start"),
        make_tool_call_msg("tc1")?,
        crate::types::Message::user("in-between"),
        make_tool_result_msg("tc1")?,
    ];

    // -- Exec & Check: a valid adjacent pair passes through unchanged
    let result = validate_tool_call_adjacency(valid_pair.clone());
    assert_msgs_eq(&result, &valid_pair);

    // -- Exec & Check: multiple valid pairs pass through unchanged
    let result = validate_tool_call_adjacency(multiple_pairs.clone());
    assert_msgs_eq(&result, &multiple_pairs);

    // -- Exec & Check: a non-adjacent pair is stripped
    let result = validate_tool_call_adjacency(non_adjacent);
    let has_tc = result
        .iter()
        .any(|m| matches!(m, crate::types::Message::Assistant(_)));
    let has_tr = result.iter().any(|m| match m {
        crate::types::Message::User { content } => content
            .iter()
            .any(|i| matches!(i, crate::types::UserContent::ToolResult(_))),
        _ => false,
    });
    assert!(!has_tc, "ToolCall must be stripped from non-adjacent pair");
    assert!(
        !has_tr,
        "ToolResult must be stripped from non-adjacent pair"
    );
    assert_eq!(result.len(), 2, "only the two non-tool messages remain");
    Ok(())
}
