use crate::commands::agent::ui::{
    event::UiEvent,
    factory::{StderrUiFactory, UiRendererFactory},
    policy::{UiPolicy, Verbosity},
    renderer::UiRenderer,
};

#[test]
fn factory_creates_renderer_without_core_loop_changes() {
    let factory = StderrUiFactory::new(Vec::<u8>::new(), false);
    let mut renderer = factory.create(UiPolicy {
        quiet: false,
        verbosity: Verbosity::Normal,
    });

    renderer.emit(&UiEvent::Tick);
    renderer.emit(&UiEvent::Completed { tool_calls: 0 });
    renderer.flush();
}
