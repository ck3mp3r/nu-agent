use agnt_core::protocol::event::UiEvent;
use agnt_core::renderer::UiRenderer;

use crate::factory::{StderrUiFactory, UiRendererFactory};
use crate::policy::{UiPolicy, Verbosity};

#[test]
fn factory_creates_renderer_without_core_loop_changes() {
    let factory = StderrUiFactory::new(Vec::<u8>::new(), false);
    let mut renderer = factory.create(UiPolicy {
        quiet: false,
        verbosity: Verbosity::Normal,
    });

    renderer.emit(&UiEvent::Tick);
    renderer.emit(&UiEvent::Completed {
        tool_calls: 0,
        input_tokens: 0,
        output_tokens: 0,
        total_tokens: None,
        cost: None,
        cost_complete: true,
        duration_ms: 0,
    });
    renderer.flush();
}
