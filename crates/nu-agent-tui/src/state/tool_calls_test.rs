use crate::state::*;
use nu_agent_core::transcript::ir::Role;
use nu_agent_core::transcript::items::TranscriptEntryKind;
use nu_agent_core::transcript::renderer::ItemStatus;

#[test]
fn tool_call_lifecycle_tracks_transcript_line_status_by_same_row() {
    let mut state = AppState::default();

    state.start_tool_call("k8s__list_pods", r#"{"namespace":"prod"}"#);

    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].role(), Role::Tool);
    assert_eq!(state.transcript_preview[0].text(), "k8s__list_pods");
    if let TranscriptEntryKind::Tool(invocation) = &state.transcript_preview[0].kind {
        assert!(invocation.args.contains("→ "));
        assert!(invocation.args.contains("namespace"));
    } else {
        panic!("Expected Tool variant");
    }
    assert_eq!(
        state.transcript_preview[0].status,
        Some(ItemStatus::InProgress)
    );

    state.finish_tool_call("k8s__list_pods", r#"{"namespace":"prod"}"#, true);
    assert_eq!(state.transcript_preview[0].status, Some(ItemStatus::Done));
}

#[test]
fn concurrent_same_name_tool_calls_get_correct_statuses() {
    let mut state = AppState::default();

    // Start two tool calls with the same name but different arguments
    state.start_tool_call("k8s__get_pod", r#"{"name":"api-0"}"#);
    state.start_tool_call("k8s__get_pod", r#"{"name":"api-1"}"#);

    // Both should be InProgress
    assert_eq!(state.transcript_preview.len(), 2);
    assert_eq!(
        state.transcript_preview[0].status,
        Some(ItemStatus::InProgress)
    );
    assert_eq!(
        state.transcript_preview[1].status,
        Some(ItemStatus::InProgress)
    );

    // Finish in reverse order
    state.finish_tool_call("k8s__get_pod", r#"{"name":"api-1"}"#, true);
    state.finish_tool_call("k8s__get_pod", r#"{"name":"api-0"}"#, false);

    // Each should get the correct status
    assert_eq!(state.transcript_preview[0].status, Some(ItemStatus::Failed));
    assert_eq!(state.transcript_preview[1].status, Some(ItemStatus::Done));
}
