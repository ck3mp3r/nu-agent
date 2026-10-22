use super::*;

/// `repair_messages` heals the real broken-session JSONL pattern and never
/// leaves a dangling ToolCall after the trailing-user trim.
#[test]
fn repair_messages_heals_session_jsonl_pattern() {
    // Simulate the real broken session pattern:
    //   user(prompt)
    //   asst([tool_call(tc_read1), tool_call(tc_read2), tool_call(tc_ls)])
    //   user([tool_result(tc_read1), tool_result(tc_read2), tool_result(tc_ls)])  ← pure ToolResult
    //   user("continue")  ← next is User → inject assistant between them
    //
    // After inject_assistant_after_dangling_tool_results, there are 5 messages.
    // Then trim_trailing_user removes the trailing user("continue") (text-only, no ToolResult),
    // yielding 4 messages. The injected assistant is at index 3 (the final message).
    let msgs = vec![
        Message::user("prompt"),
        assistant_with_content(vec![
            make_tool_call("tc_read1", "read"),
            make_tool_call("tc_read2", "read"),
            make_tool_call("tc_ls", "ls"),
        ]),
        user_with_content(vec![
            make_tool_result("tc_read1", "content1"),
            make_tool_result("tc_read2", "content2"),
            make_tool_result("tc_ls", "skills"),
        ]),
        Message::user("continue"),
    ];
    let (result, issues) = repair_messages(msgs);

    assert_eq!(
        result.len(),
        4,
        "expected 4 messages after repair; got: {result:?}"
    );
    match &result[3] {
        Message::Assistant(assistant) => {
            let content = &assistant.content;
            let text = content.iter().find_map(|c| match c {
                crate::types::AssistantContent::Text(t) => Some(t.text.as_str()),
                _ => None,
            });
            assert!(
                text.is_some_and(|t| t.contains("[interrupted")),
                "result[3] must be the injected assistant message; text: {text:?}"
            );
        }
        _ => panic!(
            "result[3] must be an Assistant message; got: {:?}",
            result[3]
        ),
    }
    assert!(
        !issues.is_empty(),
        "repair_messages must emit diagnostics for the corrupt input"
    );

    // -- Pass ordering: `trim_trailing_user` must not leave a dangling ToolCall
    let msgs = vec![
        Message::user("q"),
        assistant_with_content(vec![make_tool_call("abc", "do_thing")]),
        user_with_content(vec![make_tool_result("abc", "ok")]),
    ];
    let (result, _) = repair_messages(msgs);

    // Walk the result; for every Assistant message with ToolCalls, the very
    // next message must be a User message containing matching ToolResults.
    for (i, msg) in result.iter().enumerate() {
        let Message::Assistant(assistant) = msg else {
            continue;
        };
        let content = &assistant.content;
        let call_ids: Vec<String> = content
            .iter()
            .filter_map(|item| match item {
                AssistantContent::ToolCall(tc) => Some(tc.id.wire().into_owned()),
                _ => None,
            })
            .collect();
        if call_ids.is_empty() {
            continue;
        }
        let next = result.get(i + 1);
        let next_result_ids: Vec<String> = match next {
            Some(Message::User { content }) => content
                .iter()
                .filter_map(|item| match item {
                    UserContent::ToolResult(tr) => Some(tr.call.wire().into_owned()),
                    _ => None,
                })
                .collect(),
            _ => vec![],
        };
        for id in &call_ids {
            assert!(
                next_result_ids.contains(id),
                "ToolCall id={id} has no adjacent ToolResult in message[{}]; result length={}",
                i + 1,
                result.len()
            );
        }
    }
}
