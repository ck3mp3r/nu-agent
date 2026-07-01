use crate::types::{AssistantContent, Message, ToolResult, ToolResultContent, UserContent};

/// For each `Assistant(ToolCall)` in `messages` that has no matching `User(ToolResult)`
/// anywhere in `messages`, inserts a synthetic `User` message containing `ToolResult`
/// entries with content `"[interrupted]"` immediately after that `Assistant` message.
/// All unpaired ToolCalls from a single Assistant message are grouped into one synthetic
/// User message. Returns the patched message list.
pub fn inject_missing_tool_results(messages: Vec<Message>) -> Vec<Message> {
    use std::collections::HashSet;

    // Collect all ToolResult IDs that already exist in the history.
    let existing_result_ids: HashSet<String> = messages
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

    let mut result: Vec<Message> = Vec::with_capacity(messages.len());
    let mut patch_count: usize = 0;

    for msg in messages {
        match &msg {
            Message::Assistant { content, .. } => {
                // Collect unpaired ToolCall IDs from this Assistant message.
                let unpaired_ids: Vec<String> = content
                    .iter()
                    .filter_map(|item| match item {
                        AssistantContent::ToolCall(tc) => {
                            if !existing_result_ids.contains(&tc.id) {
                                Some(tc.id.clone())
                            } else {
                                None
                            }
                        }
                        _ => None,
                    })
                    .collect();

                result.push(msg);

                if !unpaired_ids.is_empty() {
                    // Build a single User message with one ToolResult per unpaired call.
                    let tool_results: Vec<UserContent> = unpaired_ids
                        .into_iter()
                        .map(|id| {
                            patch_count += 1;
                            UserContent::ToolResult(ToolResult {
                                id,
                                call_id: None,
                                content: rig::one_or_many::OneOrMany::one(ToolResultContent::text(
                                    "[interrupted]",
                                )),
                            })
                        })
                        .collect();

                    if let Ok(content) = rig::one_or_many::OneOrMany::many(tool_results) {
                        result.push(Message::User { content });
                    }
                }
            }
            _ => result.push(msg),
        }
    }

    if patch_count > 0 {
        log::debug!("inject_missing_tool_results: inserted {patch_count} synthetic results");
    }

    result
}

/// Detects any `User` message whose content consists entirely of `ToolResult`
/// items that is immediately followed by another `User` message (of any content).
/// Injects a synthetic `Assistant("[interrupted — turn failed before LLM responded]")`
/// between each such pair.
///
/// This heals sessions saved in a structurally invalid state where a mid-tool-loop
/// abort persisted tool results without a closing assistant message.
/// Idempotent: a second pass over already-healed history is a no-op.
pub(crate) fn inject_assistant_after_dangling_tool_results(
    messages: Vec<Message>,
    issues: &mut Vec<String>,
) -> Vec<Message> {
    let mut result: Vec<Message> = Vec::with_capacity(messages.len() + 4);
    let mut iter = messages.into_iter().peekable();
    while let Some(msg) = iter.next() {
        let is_pure_tool_result = matches!(
            &msg,
            Message::User { content } if content.iter().all(|c| matches!(c, UserContent::ToolResult(_)))
        );
        result.push(msg);
        if is_pure_tool_result && matches!(iter.peek(), Some(Message::User { .. })) {
            result.push(Message::assistant(
                "[interrupted — turn failed before LLM responded]",
            ));
            issues.push("injected synthetic assistant after dangling tool results".to_string());
        }
    }

    result
}

/// Repairs a conversation history to satisfy rig/Copilot API invariants.
///
/// Returns the repaired messages and a list of diagnostic strings describing
/// what was changed (empty if no repairs were needed).
pub fn repair_messages(messages: Vec<Message>) -> (Vec<Message>, Vec<String>) {
    let mut issues = Vec::new();
    // Pass ordering: each pass may expose issues for later passes.
    // fix_tool_call_integrity runs late so earlier passes' violations are caught.
    // fix_null_tool_arguments follows to heal null-args ToolCalls.
    // A final trim_trailing_user cleans up any trailing User(Text) exposed
    // by integrity removal or argument fixing.
    let messages = inject_assistant_after_dangling_tool_results(messages, &mut issues); // NEW — first
    let messages = remove_empty_messages(messages, &mut issues);
    let messages = merge_consecutive_same_role(messages, &mut issues);
    let messages = trim_trailing_user(messages, &mut issues);
    let messages = fix_tool_call_integrity(messages, &mut issues);
    let messages = fix_null_tool_arguments(messages, &mut issues);
    let messages = trim_trailing_user(messages, &mut issues);
    let messages = ensure_valid(messages, &mut issues);
    let messages = fix_empty_tool_results(messages, &mut issues);
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

/// Replaces `null` arguments in ToolCall messages with `{}` (empty JSON object).
/// Heals sessions poisoned before the `on_invalid_tool_call` → Retry migration.
/// Also guards against any future path that may persist a null-args ToolCall.
pub(crate) fn fix_null_tool_arguments(
    messages: Vec<Message>,
    issues: &mut Vec<String>,
) -> Vec<Message> {
    messages
        .into_iter()
        .map(|msg| match msg {
            Message::Assistant { id, content } => {
                let mut fixed = false;
                let new_content: Vec<_> = content
                    .into_iter()
                    .map(|c| match c {
                        AssistantContent::ToolCall(mut tc) if tc.function.arguments.is_null() => {
                            tc.function.arguments = serde_json::json!({});
                            fixed = true;
                            AssistantContent::ToolCall(tc)
                        }
                        other => other,
                    })
                    .collect();
                if fixed {
                    issues.push("replaced null tool call arguments with {}".to_string());
                }
                Message::Assistant {
                    id,
                    content: rig::one_or_many::OneOrMany::many(new_content)
                        .expect("non-empty assistant content"),
                }
            }
            other => other,
        })
        .collect()
}

pub(crate) fn fix_tool_call_integrity(
    messages: Vec<Message>,
    issues: &mut Vec<String>,
) -> Vec<Message> {
    use std::collections::HashSet;

    // We loop until no violations remain; stripping one pair may expose another.
    let mut messages = messages;
    loop {
        // --- Step 1: orphan removal (global ID matching) ---

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

        let after_orphan: Vec<Message> = messages
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
                                "removed user message emptied by ToolResult integrity pass"
                                    .to_string(),
                            );
                            None
                        }
                    }
                }
                system @ Message::System { .. } => Some(system),
            })
            .collect();

        // --- Step 2: adjacency enforcement ---
        // Find the first Assistant message whose ToolCall IDs are not all
        // covered by the immediately following User message.
        let violation_ids: Option<HashSet<String>> =
            after_orphan.iter().enumerate().find_map(|(i, msg)| {
                let Message::Assistant { content, .. } = msg else {
                    return None;
                };
                let call_ids: HashSet<String> = content
                    .iter()
                    .filter_map(|item| match item {
                        AssistantContent::ToolCall(tc) => Some(tc.id.clone()),
                        _ => None,
                    })
                    .collect();
                if call_ids.is_empty() {
                    return None;
                }
                let next_result_ids: HashSet<String> = match after_orphan.get(i + 1) {
                    Some(Message::User { content }) => content
                        .iter()
                        .filter_map(|item| match item {
                            UserContent::ToolResult(tr) => Some(tr.id.clone()),
                            _ => None,
                        })
                        .collect(),
                    _ => HashSet::new(),
                };
                if call_ids.is_subset(&next_result_ids) {
                    None
                } else {
                    Some(call_ids)
                }
            });

        match violation_ids {
            None => {
                // No violations; we are done.
                messages = after_orphan;
                break;
            }
            Some(bad_ids) => {
                // Strip the offending Assistant message and all ToolResult entries
                // with those IDs from anywhere in the list, then loop again.
                for id in &bad_ids {
                    issues.push(format!(
                        "removed non-adjacent ToolCall/ToolResult pair id={}",
                        id
                    ));
                }
                messages = after_orphan
                    .into_iter()
                    .filter_map(|msg| match msg {
                        Message::Assistant { id, content } => {
                            let items: Vec<AssistantContent> = content
                                .into_iter()
                                .filter(|item| match item {
                                    AssistantContent::ToolCall(tc) => !bad_ids.contains(&tc.id),
                                    _ => true,
                                })
                                .collect();
                            match rig::one_or_many::OneOrMany::many(items) {
                                Ok(content) => Some(Message::Assistant { id, content }),
                                Err(_) => None,
                            }
                        }
                        Message::User { content } => {
                            let items: Vec<UserContent> = content
                                .into_iter()
                                .filter(|item| match item {
                                    UserContent::ToolResult(tr) => !bad_ids.contains(&tr.id),
                                    _ => true,
                                })
                                .collect();
                            match rig::one_or_many::OneOrMany::many(items) {
                                Ok(content) => Some(Message::User { content }),
                                Err(_) => None,
                            }
                        }
                        system @ Message::System { .. } => Some(system),
                    })
                    .collect();
            }
        }
    }

    messages
}

pub(crate) fn merge_consecutive_same_role(
    messages: Vec<Message>,
    issues: &mut Vec<String>,
) -> Vec<Message> {
    let mut result: Vec<Message> = Vec::with_capacity(messages.len());

    for msg in messages {
        match (result.last_mut(), msg) {
            (Some(Message::User { content: prev }), Message::User { content: next })
                if !prev.iter().any(|i| matches!(i, UserContent::ToolResult(_)))
                    && !next.iter().any(|i| matches!(i, UserContent::ToolResult(_))) =>
            {
                issues.push("merged consecutive user messages".to_string());
                for item in next {
                    prev.push(item);
                }
            }
            // Fix 3: guard Assistant merge — do NOT merge if either message
            // contains a ToolCall.
            (
                Some(Message::Assistant { content: prev, .. }),
                Message::Assistant { content: next, .. },
            ) if !prev
                .iter()
                .any(|i| matches!(i, AssistantContent::ToolCall(_)))
                && !next
                    .iter()
                    .any(|i| matches!(i, AssistantContent::ToolCall(_))) =>
            {
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
    // Fix 1: do NOT pop a User message that contains any ToolResult content.
    while let Some(Message::User { content }) = messages.last() {
        if content
            .iter()
            .any(|i| matches!(i, UserContent::ToolResult(_)))
        {
            break;
        }
        messages.pop();
        issues.push("trimmed trailing orphan user message".to_string());
    }
    messages
}

fn ensure_valid(messages: Vec<Message>, _issues: &mut Vec<String>) -> Vec<Message> {
    // An empty history is valid — rig handles it as a fresh conversation.
    messages
}

/// Replaces any `ToolResult` within a `User` message whose content consists
/// entirely of empty (blank) text with a single `"(empty result)"` placeholder.
///
/// Empty tool results trigger API validation errors on some providers.
/// This pass is applied **last** in `repair_messages` so that earlier passes
/// (which may synthesise tool results) can always produce a non-empty string.
pub(crate) fn fix_empty_tool_results(
    messages: Vec<Message>,
    issues: &mut Vec<String>,
) -> Vec<Message> {
    messages
        .into_iter()
        .map(|msg| match msg {
            Message::User { content } => {
                let fixed: Vec<UserContent> = content
                    .into_iter()
                    .map(|item| match item {
                        UserContent::ToolResult(mut tr) => {
                            let has_empty = tr.content.iter().any(|c| {
                                matches!(
                                    c,
                                    crate::types::ToolResultContent::Text(t)
                                        if t.text.trim().is_empty()
                                )
                            });
                            if has_empty {
                                issues.push(format!(
                                    "replaced empty tool_result content id={}",
                                    tr.id
                                ));
                                tr.content = rig::one_or_many::OneOrMany::one(
                                    crate::types::ToolResultContent::text("(empty result)"),
                                );
                            }
                            UserContent::ToolResult(tr)
                        }
                        other => other,
                    })
                    .collect();
                Message::User {
                    content: rig::one_or_many::OneOrMany::many(fixed).unwrap_or_else(|_| {
                        rig::one_or_many::OneOrMany::one(UserContent::Text(crate::types::Text {
                            text: "(empty result)".to_string(),
                            additional_params: None,
                        }))
                    }),
                }
            }
            other => other,
        })
        .collect()
}
