use std::sync::{Arc, Mutex};

use crate::commands::agent::ui::{
    event::UiEvent,
    policy::{UiPolicy, Verbosity},
    renderer::UiRenderer,
};

#[derive(Clone)]
struct FakeRenderer {
    events: Arc<Mutex<Vec<String>>>,
}

impl FakeRenderer {
    fn new(_policy: UiPolicy) -> Self {
        Self {
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().expect("events").clone()
    }

}

impl UiRenderer for FakeRenderer {
    fn emit(&mut self, event: &UiEvent) {
        let label = match event {
            UiEvent::LlmStart => "llm_start",
            UiEvent::Tick => "tick",
            UiEvent::LlmEnd { .. } => "llm_end",
            UiEvent::ToolStart { .. } => "tool_start",
            UiEvent::ToolEnd { .. } => "tool_end",
            UiEvent::Warning { .. } => "warning",
            UiEvent::Completed { .. } => "completed",
        };
        self.events.lock().expect("events").push(label.to_string());
    }

    fn flush(&mut self) {}
}

fn run_mock_flow<R: UiRenderer>(renderer: &mut R) {
    renderer.emit(&UiEvent::LlmStart);
    renderer.emit(&UiEvent::Tick);
    renderer.emit(&UiEvent::LlmEnd {
        response_chars: 3,
        tool_calls: 1,
    });
    renderer.emit(&UiEvent::ToolStart {
        name: "t".to_string(),
        source: "closure".to_string(),
        arguments: "{}".to_string(),
    });
    renderer.emit(&UiEvent::ToolEnd {
        name: "t".to_string(),
        source: "closure".to_string(),
        arguments: "{}".to_string(),
        success: true,
        result: "ok".to_string(),
        error_kind: None,
        message: None,
    });
    renderer.emit(&UiEvent::Completed { tool_calls: 1 });
    renderer.flush();
}

#[test]
fn renderer_is_substitutable_via_trait_boundary() {
    let mut fake = FakeRenderer::new(UiPolicy {
        quiet: false,
        verbosity: Verbosity::Normal,
    });
    run_mock_flow(&mut fake);
    assert_eq!(
        fake.events(),
        vec![
            "llm_start",
            "tick",
            "llm_end",
            "tool_start",
            "tool_end",
            "completed"
        ]
    );
}
