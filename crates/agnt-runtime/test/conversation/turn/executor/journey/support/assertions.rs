use super::harness::Result;
use crate::types::Message;

// ---------------------------------------------------------------------------
// Assertion helpers
// ---------------------------------------------------------------------------

pub(crate) fn assert_user_text(msg: &Message, expected: &str) {
    let Message::User { content } = msg else {
        panic!("expected User message, got: {msg:?}");
    };
    let text = content.iter().find_map(|c| {
        if let rig::message::UserContent::Text(t) = c {
            Some(t.text.as_str())
        } else {
            None
        }
    });
    assert_eq!(text, Some(expected), "user text mismatch");
}

pub(crate) fn assert_assistant_text_contains(msg: &Message, needle: &str) {
    let Message::Assistant(assistant) = msg else {
        panic!("expected Assistant message, got: {msg:?}");
    };
    let content = &assistant.content;
    let text = content.iter().find_map(|c| {
        if let rig::message::AssistantContent::Text(t) = c {
            Some(t.text.as_str())
        } else {
            None
        }
    });
    assert!(
        text.is_some_and(|t| t.contains(needle)),
        "assistant text {text:?} does not contain {needle:?}"
    );
}

pub(crate) fn assert_tool_call_in_msg(
    msg: &Message,
    expected_id: &str,
    expected_name: &str,
) -> Result<()> {
    let Message::Assistant(assistant) = msg else {
        panic!("expected Assistant message for tool call, got: {msg:?}");
    };
    let content = &assistant.content;
    let tc = content.iter().find_map(|c| {
        if let rig::message::AssistantContent::ToolCall(tc) = c {
            Some(tc)
        } else {
            None
        }
    });
    let tc = tc.ok_or("no ToolCall content in Assistant message")?;
    assert_eq!(tc.id.wire().as_ref(), expected_id, "tool call id mismatch");
    assert_eq!(tc.function.name, expected_name, "tool call name mismatch");
    Ok(())
}

pub(crate) fn assert_tool_result_in_msg(
    msg: &Message,
    expected_id: &str,
    content_contains: &str,
) -> Result<()> {
    let Message::User { content } = msg else {
        panic!("expected User message for tool result, got: {msg:?}");
    };
    let tr = content
        .iter()
        .find_map(|c| {
            if let rig::message::UserContent::ToolResult(tr) = c {
                (tr.call.wire().as_ref() == expected_id).then_some(tr)
            } else {
                None
            }
        })
        .ok_or_else(|| {
            Box::<dyn std::error::Error>::from(format!(
                "no ToolResult for call id {expected_id} in User message"
            ))
        })?;
    let content_str = format!("{:?}", tr.content);
    assert!(
        content_str.contains(content_contains),
        "ToolResult content {content_str:?} does not contain {content_contains:?}"
    );
    Ok(())
}

/// Assert the ToolResult whose call id is `expected_id` carries `expected`
/// as its persisted verdict (`ToolResult::is_error` inverted).
pub(crate) fn assert_tool_result_flag(
    msg: &Message,
    expected_id: &str,
    expected: Option<bool>,
) -> Result<()> {
    let Message::User { content } = msg else {
        panic!("expected User message for tool result, got: {msg:?}");
    };
    let tr = content
        .iter()
        .find_map(|c| {
            if let rig::message::UserContent::ToolResult(tr) = c {
                (tr.call.wire().as_ref() == expected_id).then_some(tr)
            } else {
                None
            }
        })
        .ok_or_else(|| {
            Box::<dyn std::error::Error>::from(format!(
                "no ToolResult for call id {expected_id} in User message"
            ))
        })?;
    let flag = Some(!tr.is_error);
    assert_eq!(
        flag, expected,
        "persisted verdict mismatch for {expected_id}"
    );
    Ok(())
}

pub(crate) fn assert_no_interrupted(msgs: &[Message]) {
    for msg in msgs {
        if let Message::User { content } = msg {
            for c in content.iter() {
                if let rig::message::UserContent::ToolResult(tr) = c {
                    let s = format!("{:?}", tr.content);
                    assert!(
                        !s.contains("[interrupted]"),
                        "synthetic [interrupted] found in ToolResult {tr:?}"
                    );
                }
            }
        }
    }
}
