use crate::renderer::{RenderInput, UiRenderer};
use agnt_kernel::{SessionRecord, TurnRecord};

use crate::factory::{StderrUiFactory, UiRendererFactory};
use crate::policy::{UiPolicy, Verbosity};

#[test]
fn factory_creates_renderer_without_core_loop_changes() {
    let factory = StderrUiFactory::new(Vec::<u8>::new(), false);
    let mut renderer = factory.create(UiPolicy {
        quiet: false,
        verbosity: Verbosity::Normal,
    });

    renderer.tick();
    let record = SessionRecord::Turn(TurnRecord::Completed { duration_ms: 0 });
    renderer.draw(RenderInput::Record(&record));
    renderer.flush();
}
