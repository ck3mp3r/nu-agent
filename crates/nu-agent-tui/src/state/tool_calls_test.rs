use crate::state::*;
use nu_agent_core::transcript::ir::Role;
use nu_agent_core::transcript::items::TranscriptEntry;

#[test]
fn tool_call_lifecycle_tracks_transcript_line_status_by_same_row() {
    let mut state = AppState::new();

    state.start_tool_call("k8s__list_pods", r#"{"namespace":"prod"}"#);

    assert_eq!(state.transcript_preview.len(), 1);
    assert_eq!(state.transcript_preview[0].role(), Role::Tool);
    assert_eq!(state.transcript_preview[0].text(), "k8s__list_pods");
    if let TranscriptEntry::Tool(invocation) = &state.transcript_preview[0] {
        assert!(invocation.args.contains("→ "));
        assert!(invocation.args.contains("namespace"));
    } else {
        panic!("Expected Tool variant");
    }
    assert_eq!(
        state.transcript_line_status_for_index(0),
        Some(TranscriptLineStatus::Tool(ToolCallStatus::InProgress))
    );

    state.finish_tool_call("k8s__list_pods", r#"{"namespace":"prod"}"#, true);
    assert_eq!(
        state.transcript_line_status_for_index(0),
        Some(TranscriptLineStatus::Tool(ToolCallStatus::Done))
    );
}
