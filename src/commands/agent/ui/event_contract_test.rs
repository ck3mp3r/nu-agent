use crate::commands::agent::ui::event::UiEvent;

#[test]
fn ui_event_contract_exposes_required_variants() {
    let events = [
        UiEvent::LlmStart,
        UiEvent::Tick,
        UiEvent::LlmEnd {
            response_chars: 12,
            tool_calls: 1,
        },
        UiEvent::ToolStart {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
        },
        UiEvent::ToolEnd {
            name: "k8s__list_pods".to_string(),
            source: "mcp".to_string(),
            arguments: "{}".to_string(),
            success: true,
            result: "[]".to_string(),
            error_kind: None,
            message: None,
        },
        UiEvent::Warning {
            message: "compaction failed".to_string(),
        },
        UiEvent::Completed { tool_calls: 1 },
    ];

    assert_eq!(events.len(), 7);
}
