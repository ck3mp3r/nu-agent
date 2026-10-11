pub mod ansi;
pub mod factory;
pub mod formatter;
pub mod markdown_buffer;
pub mod policy;
pub mod progress;
pub mod renderer;
pub mod spinner;

pub use factory::{StderrUiFactory, UiRendererFactory};
pub use policy::{UiPolicy, Verbosity};
pub use progress::StderrProgressUi;
pub use renderer::tty::layout;

#[cfg(test)]
#[path = "../test/factory.rs"]
mod factory_test;
#[cfg(test)]
#[path = "../test/formatter.rs"]
mod formatter_test;
#[cfg(test)]
#[path = "../test/progress.rs"]
mod progress_test;
#[cfg(test)]
#[path = "../test/spinner.rs"]
mod spinner_test;
