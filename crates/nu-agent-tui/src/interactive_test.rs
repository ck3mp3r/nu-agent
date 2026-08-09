use std::time::Duration;

use nu_agent_core::protocol::contracts::DisplayStateUi;
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
fn set_active_persona_icon_updates_state_through_display_state_ui() {
    let mut ui = make_ui();

    <TuiInteractiveUi<FakeRenderer> as DisplayStateUi>::set_active_persona_icon(
        &mut ui,
        Some("icon".to_string()),
    );

    assert_eq!(
        ui.renderer
            .coordinator()
            .state()
            .active_persona_icon
            .as_deref(),
        Some("icon")
    );
}

#[test]
fn set_active_persona_icon_clears_state_when_none() {
    let mut ui = make_ui();

    <TuiInteractiveUi<FakeRenderer> as DisplayStateUi>::set_active_persona_icon(
        &mut ui,
        Some("icon".to_string()),
    );
    <TuiInteractiveUi<FakeRenderer> as DisplayStateUi>::set_active_persona_icon(&mut ui, None);

    assert_eq!(ui.renderer.coordinator().state().active_persona_icon, None);
}
