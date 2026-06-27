use crate::types::{AssistantContent, Message, UserContent};

/// Repairs a conversation history to satisfy rig/Copilot API invariants.
///
/// Returns the repaired messages and a list of diagnostic strings describing
/// what was changed (empty if no repairs were needed).
pub fn repair_messages(messages: Vec<Message>) -> (Vec<Message>, Vec<String>) {
    let mut issues = Vec::new();
    let messages = remove_empty_messages(messages, &mut issues);
    let messages = fix_tool_call_integrity(messages, &mut issues);
    let messages = merge_consecutive_same_role(messages, &mut issues);
    let messages = trim_trailing_user(messages, &mut issues);
    let messages = ensure_valid(messages, &mut issues);
    (messages, issues)
}

pub(crate) fn remove_empty_messages(
    messages: Vec<Message>,
    issues: &mut Vec<String>,
) -> Vec<Message> {
    messages
        .into_iter()
        .filter(|msg| {
            let keep = !is_message_empty(msg);
            if !keep {
                issues.push(format!("removed empty {:?} message", role_name(msg)));
            }
            keep
        })
        .collect()
}

fn is_message_empty(msg: &Message) -> bool {
    match msg {
        Message::System { content } => content.trim().is_empty(),
        Message::User { content } => {
            content.iter().all(|item| match item {
                UserContent::Text(t) => t.text.trim().is_empty(),
                // ToolResult counts as non-empty
                _ => false,
            })
        }
        Message::Assistant { content, .. } => {
            content.iter().all(|item| match item {
                AssistantContent::Text(t) => t.text.trim().is_empty(),
                // ToolCall counts as non-empty
                _ => false,
            })
        }
    }
}

fn role_name(msg: &Message) -> &'static str {
    match msg {
        Message::System { .. } => "system",
        Message::User { .. } => "user",
        Message::Assistant { .. } => "assistant",
    }
}

pub(crate) fn fix_tool_call_integrity(
    messages: Vec<Message>,
    issues: &mut Vec<String>,
) -> Vec<Message> {
    use std::collections::HashSet;

    // Collect all ToolCall ids from assistant messages.
    let all_call_ids: HashSet<String> = messages
        .iter()
        .flat_map(|msg| match msg {
            Message::Assistant { content, .. } => content
                .iter()
                .filter_map(|item| match item {
                    AssistantContent::ToolCall(tc) => Some(tc.id.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => vec![],
        })
        .collect();

    // Collect all ToolResult ids from user messages.
    let all_result_ids: HashSet<String> = messages
        .iter()
        .flat_map(|msg| match msg {
            Message::User { content } => content
                .iter()
                .filter_map(|item| match item {
                    UserContent::ToolResult(tr) => Some(tr.id.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            _ => vec![],
        })
        .collect();

    messages
        .into_iter()
        .filter_map(|msg| match msg {
            Message::Assistant { id, content } => {
                let items: Vec<AssistantContent> = content
                    .into_iter()
                    .filter(|item| match item {
                        AssistantContent::ToolCall(tc) => {
                            let matched = all_result_ids.contains(&tc.id);
                            if !matched {
                                issues.push(format!(
                                    "removed dangling ToolCall id={} (no matching ToolResult)",
                                    tc.id
                                ));
                            }
                            matched
                        }
                        _ => true,
                    })
                    .collect();

                match rig::one_or_many::OneOrMany::many(items) {
                    Ok(content) => Some(Message::Assistant { id, content }),
                    Err(_) => {
                        issues.push(
                            "removed assistant message emptied by ToolCall integrity pass"
                                .to_string(),
                        );
                        None
                    }
                }
            }
            Message::User { content } => {
                let items: Vec<UserContent> = content
                    .into_iter()
                    .filter(|item| match item {
                        UserContent::ToolResult(tr) => {
                            let matched = all_call_ids.contains(&tr.id);
                            if !matched {
                                issues.push(format!(
                                    "removed orphaned ToolResult id={} (no matching ToolCall)",
                                    tr.id
                                ));
                            }
                            matched
                        }
                        _ => true,
                    })
                    .collect();

                match rig::one_or_many::OneOrMany::many(items) {
                    Ok(content) => Some(Message::User { content }),
                    Err(_) => {
                        issues.push(
                            "removed user message emptied by ToolResult integrity pass".to_string(),
                        );
                        None
                    }
                }
            }
            system @ Message::System { .. } => Some(system),
        })
        .collect()
}

pub(crate) fn merge_consecutive_same_role(
    messages: Vec<Message>,
    issues: &mut Vec<String>,
) -> Vec<Message> {
    let mut result: Vec<Message> = Vec::with_capacity(messages.len());

    for msg in messages {
        match (result.last_mut(), msg) {
            (Some(Message::User { content: prev }), Message::User { content: next }) => {
                issues.push("merged consecutive user messages".to_string());
                for item in next {
                    prev.push(item);
                }
            }
            (
                Some(Message::Assistant { content: prev, .. }),
                Message::Assistant { content: next, .. },
            ) => {
                issues.push("merged consecutive assistant messages".to_string());
                for item in next {
                    prev.push(item);
                }
            }
            (_, msg) => result.push(msg),
        }
    }

    result
}

pub(crate) fn trim_trailing_user(
    mut messages: Vec<Message>,
    issues: &mut Vec<String>,
) -> Vec<Message> {
    while matches!(messages.last(), Some(Message::User { .. })) {
        messages.pop();
        issues.push("trimmed trailing orphan user message".to_string());
    }
    messages
}

fn ensure_valid(messages: Vec<Message>, _issues: &mut Vec<String>) -> Vec<Message> {
    // An empty history is valid — rig handles it as a fresh conversation.
    messages
}
