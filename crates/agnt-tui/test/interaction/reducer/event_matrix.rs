use super::*;

use agnt_render::transcript::ir::ToolName;
use agnt_runtime::protocol::events::{
    NarrationEvent, PermissionRecord, RequestId, SessionRecord, ToolCall, ToolOutcome, Usage,
};
use agnt_runtime::session::tool_status::ToolStatus;

/// One matrix row's fold input: a stream record or a narration event.
///
/// The record is boxed because a record is much larger than a narration event.
enum CaseEvent {
    Record(Box<SessionRecord>),
    Narration(NarrationEvent),
}

/// A matrix row's stream record.
fn record(record: SessionRecord) -> CaseEvent {
    CaseEvent::Record(Box::new(record))
}

#[test]
fn table_driven_ui_event_matrix_covers_all_variants() {
    struct Case {
        name: &'static str,
        event: CaseEvent,
        pre: fn() -> AppState,
    }

    fn idle() -> AppState {
        AppState::default()
    }

    fn busy_empty_status() -> AppState {
        let mut state = busy_state_with_clean_transcript();
        state.status.message.clear();
        state
    }

    fn busy_with_status() -> AppState {
        let mut state = busy_state_with_clean_transcript();
        state.status.message.set_message("Tool: prior");
        state
    }

    fn busy_with_running_tool_line() -> AppState {
        let mut state = busy_state_with_clean_transcript();
        reduce_ui(
            &mut state,
            tool_started(ToolCall::new(
                ToolName("k8s__list_pods".to_string()),
                agnt_runtime::tools::handler::ToolSource::Mcp,
                r#"{"namespace":"prod"}"#,
            )),
        );
        state
    }

    let cases = vec![
        Case {
            name: "llm_start_from_idle_moves_busy",
            event: record(inference_started()),
            pre: idle,
        },
        Case {
            name: "llm_start_when_busy_is_noop",
            event: record(inference_started()),
            pre: busy_with_status,
        },
        Case {
            name: "tool_start_leaves_status_empty",
            event: record(tool_started(ToolCall::new(
                ToolName("k8s__list_pods".to_string()),
                agnt_runtime::tools::handler::ToolSource::Mcp,
                "{}",
            ))),
            pre: busy_empty_status,
        },
        Case {
            name: "tool_end_updates_tool_line_without_status",
            event: record(tool_completed(
                ToolCall::new(
                    ToolName("k8s__list_pods".to_string()),
                    agnt_runtime::tools::handler::ToolSource::Mcp,
                    r#"{"namespace":"prod"}"#,
                ),
                ToolOutcome {
                    status: ToolStatus::Success,
                    payload: None,
                    text: "[]".to_string(),
                    message: None,
                    kind: None,
                },
            )),
            pre: busy_with_running_tool_line,
        },
        Case {
            name: "llm_end_records_tokens_without_status_message",
            event: record(SessionRecord::Inference(
                agnt_runtime::protocol::events::InferenceRecord::Completed {
                    usage: Usage {
                        input_tokens: 4,
                        output_tokens: 8,
                        total_tokens: Some(12),
                        cost: None,
                        cost_complete: false,
                    },
                },
            )),
            pre: busy_empty_status,
        },
        Case {
            name: "warning_sets_status_line",
            event: CaseEvent::Narration(system_message(
                agnt_runtime::protocol::events::NoticeKind::Warning,
                "warned",
            )),
            pre: busy_empty_status,
        },
        Case {
            name: "assistant_message_trims_and_appends",
            event: CaseEvent::Narration(inference_chunk(
                "\nline 1\nline 2\n",
                "\nline 1\nline 2\n",
            )),
            pre: busy_empty_status,
        },
        Case {
            name: "completed_finalizes_cycle",
            event: record(turn_completed(0)),
            pre: busy_with_status,
        },
    ];

    for case in cases {
        let mut state = (case.pre)();
        match case.event {
            CaseEvent::Record(record) => {
                reduce_ui(&mut state, (*record).clone());
            }
            CaseEvent::Narration(event) => {
                reduce_narration(&mut state, event);
            }
        }

        match case.name {
            "llm_start_from_idle_moves_busy" => {
                assert_eq!(state.phase, UiPhase::Busy);
                assert!(state.input_locked);
            }
            "llm_start_when_busy_is_noop" => {
                assert_eq!(state.phase, UiPhase::Busy);
                assert_eq!(state.status.message.status_line(), "Tool: prior");
                assert!(state.input_locked);
            }
            "tick_leaves_empty_status_empty" => {
                assert!(state.status.message.status_line().is_empty());
            }
            "tick_preserves_existing_status" => {
                assert_eq!(state.status.message.status_line(), "Tool: prior");
            }
            "tool_start_leaves_status_empty" => {
                assert!(state.status.message.status_line().is_empty());
            }
            "tool_end_updates_tool_line_without_status" => {
                // [Tool] — no leading spacer under the unified spacer rule
                assert_eq!(state.transcript.len(), 1);
                assert!(matches!(
                    &state.transcript.entries()[0],
                    TranscriptEntry::Tool(_)
                ));
                assert!(state.status.message.status_line().is_empty());
            }
            "llm_end_records_tokens_without_status_message" => {
                assert_eq!(state.status.tokens.latest_input_tokens, Some(4));
                assert_eq!(state.status.tokens.latest_output_tokens, Some(8));
                assert_eq!(state.status.tokens.latest_total_tokens, Some(12));
                assert_eq!(state.status.tokens.session_total_tokens, 12);
                assert!(state.status.message.status_line().is_empty());
            }
            "warning_sets_status_line" => {
                // Warning is narrated, and the narration fold routes it to the
                // status line.
                assert_eq!(state.status.message.status_line(), "warned");
            }
            "assistant_message_trims_and_appends" => {
                // After the raw-markdown refactor, a single AssistantMessage
                // produces one message entry. The raw text is trimmed before
                // storage, so leading/trailing whitespace is dropped.
                let assistant_entries: Vec<String> = state
                    .transcript
                    .entries()
                    .iter()
                    .filter(|entry| {
                        matches!(
                            entry,
                            TranscriptEntry::Message(message)
                                if message.role == MessageRole::Assistant
                        )
                    })
                    .map(entry_text)
                    .collect();
                assert_eq!(assistant_entries.len(), 1, "one assistant entry");
                let text = &assistant_entries[0];
                assert!(text.contains("line 1"), "raw md should contain 'line 1'");
                assert!(text.contains("line 2"), "raw md should contain 'line 2'");
            }
            "completed_finalizes_cycle" => {
                assert_eq!(state.phase, UiPhase::Idle);
                assert!(!state.input_locked);
                assert!(!state.abort.pending);
                assert!(state.status.message.status_line().is_empty());
            }
            _ => unreachable!("unknown case: {}", case.name),
        }

        assert_reducer_invariants(&state);
    }
}

#[test]
fn permission_request_table() -> Result<()> {
    // A permission request focuses the transcript for immediate prompt visibility.
    let mut state = AppState::default();
    state.scroll.pane_focus = crate::state::PaneFocus::Input;

    let request = SessionRecord::Permission(PermissionRecord::Requested {
        request_id: RequestId::from_minted("ask-0000000000000001"),
        call_id: agnt_runtime::protocol::events::ToolCallId::from_key(
            "edit\n{\"file\":\"foo.rs\"}",
        ),
        tool: "edit(file=foo.rs)".to_string(),
        source: "closure".to_string(),
        mode: Some("apply".to_string()),
        rule_identity: "tool:edit".to_string(),
        scope: "tool".to_string(),
        target_field: None,
        pattern: "edit".to_string(),
        summary: "→ {...}".to_string(),
        shows_preview: false,
        preview: None,
    });
    reduce_ui(&mut state, request);

    assert_eq!(state.scroll.pane_focus, crate::state::PaneFocus::Input);
    assert!(state.permission.has_prompt());

    // PermissionRequested dispatch orders the tool before the diff preview and follows the tail.
    let mut state = AppState::default();
    state.scroll.following_tail = false;

    reduce_ui(
        &mut state,
        tool_started(ToolCall::new(
            ToolName("edit".to_string()),
            agnt_runtime::tools::handler::ToolSource::Builtin,
            r#"{"path":"test.rs"}"#,
        )),
    );
    reduce_ui(
        &mut state,
        SessionRecord::Permission(PermissionRecord::Requested {
            request_id: RequestId::from_minted("perm-1"),
            call_id: agnt_runtime::protocol::events::ToolCallId::from_key(
                "edit\n{\"path\":\"test.rs\"}",
            ),
            tool: "edit".to_string(),
            source: "builtin".to_string(),
            mode: None,
            rule_identity: "tool:edit".to_string(),
            scope: "tool".to_string(),
            target_field: None,
            pattern: "edit".to_string(),
            summary: "test".to_string(),
            shows_preview: true,
            preview: Some("--- a\n+++ b\n".to_string()),
        }),
    );

    // The preview lands on the pending tool entry in place, so there is
    // exactly one entry.
    assert_eq!(
        state.transcript.len(),
        1,
        "the preview must live on the tool entry"
    );
    let entry = state
        .transcript
        .entries()
        .first()
        .ok_or("tool entry should exist")?;
    let TranscriptEntry::Tool(tool) = entry else {
        return Err("expected Tool entry".into());
    };
    assert!(
        tool.preview.is_some(),
        "the permission request display must land on the entry's preview"
    );
    assert!(
        state.permission.has_prompt(),
        "permission prompt must be open after PermissionRequested"
    );
    assert!(
        state.scroll.following_tail,
        "dispatch must scroll transcript to bottom"
    );
    Ok(())
}

#[test]
fn warning_dispatch_sets_status_line() -> Result<()> {
    // -- Setup & Fixtures
    let mut state = AppState::default();

    // -- Exec
    reduce_narration(
        &mut state,
        system_message(
            agnt_runtime::protocol::events::NoticeKind::Warning,
            "test warning",
        ),
    );

    // -- Check
    assert_eq!(
        state.status.message.status_line(),
        "test warning",
        "warning dispatch must set the status line"
    );
    Ok(())
}
