use std::time::Duration;

use nu_agent_core::protocol::event::UiEvent;
use nu_agent_core::renderer::UiRenderer;

use crate::runtime::HybridTerminalEvents;

use super::TuiInteractiveUi;

struct FakeRenderer;

impl UiRenderer for FakeRenderer {
    fn emit(&mut self, _event: &UiEvent) {}
    fn flush(&mut self) {}
}

fn make_ui() -> TuiInteractiveUi<FakeRenderer> {
    let events = HybridTerminalEvents::new(Duration::from_millis(60), None);
    let renderer = crate::runtime::TuiRuntimeRenderer::new(FakeRenderer, events, 120, 30);
    TuiInteractiveUi::new(renderer)
}

#[test]
fn set_active_persona_icon_updates_state() {
    let mut ui = make_ui();

    ui.set_active_persona_icon(Some("icon".to_string()));

    assert_eq!(
        ui.renderer.coordinator.state.active_persona_icon.as_deref(),
        Some("icon")
    );
}

#[test]
fn set_active_persona_icon_clears_state_when_none() {
    let mut ui = make_ui();

    ui.set_active_persona_icon(Some("icon".to_string()));
    ui.set_active_persona_icon(None);

    assert_eq!(ui.renderer.coordinator.state.active_persona_icon, None);
}

#[test]
fn permission_event_requested_opens_permission_prompt() {
    let mut ui = make_ui();
    let event = nu_agent_core::bus::PermissionEvent::Requested {
        request_id: "ask-0000000000000001".to_string(),
        context: Box::new(nu_agent_core::protocol::event::PermissionRequestContext {
            tool: "nu".to_string(),
            source: "closure".to_string(),
            mode: Some("apply".to_string()),
            matched_rule_identity: "nested:nu.command:*".to_string(),
            scope: "nested".to_string(),
            target_field: Some("command".to_string()),
            pattern: "*".to_string(),
            summary: "→ {\"command\":\"echo hi\"}".to_string(),
            pre_authorize_display: None,
        }),
    };
    let ui_event = Option::<UiEvent>::from(event).expect("PermissionEvent converts to UiEvent");
    ui.renderer.coordinator.state.reduce_ui_event(ui_event);
    assert!(ui.renderer.coordinator.state.has_permission_prompt());
}
