use std::io::Write;

use super::{policy::UiPolicy, renderer::UiRenderer, stderr::StderrUiRenderer};

pub trait UiRendererFactory {
    type Renderer: UiRenderer;

    fn create(self, policy: UiPolicy) -> Self::Renderer;
}

pub struct StderrUiFactory<W: Write> {
    writer: W,
    stderr_is_tty: bool,
}

impl<W: Write> StderrUiFactory<W> {
    pub fn new(writer: W, stderr_is_tty: bool) -> Self {
        Self {
            writer,
            stderr_is_tty,
        }
    }
}

impl<W: Write> UiRendererFactory for StderrUiFactory<W> {
    type Renderer = StderrUiRenderer<W>;

    fn create(self, policy: UiPolicy) -> Self::Renderer {
        StderrUiRenderer::new(self.writer, policy, self.stderr_is_tty)
    }
}
