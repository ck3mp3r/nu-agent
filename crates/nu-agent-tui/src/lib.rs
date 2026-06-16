pub mod interaction;
pub mod interactive;
pub mod markdown;
pub mod platform;
pub mod rendering;
pub mod runtime;
pub mod state;
pub mod tui_renderer;

#[cfg(test)]
mod tui_renderer_test;

#[cfg(test)]
pub(crate) mod test_support;

// Re-export primary public types
pub use interactive::TuiInteractiveUi;
pub use tui_renderer::TuiRenderer;

// Re-export runtime types needed by the plugin crate
pub use runtime::{
    AnsiTerminalBackend, HybridTerminalEvents, RuntimeRunError, TuiRuntimeRenderer,
    run_with_terminal_restore,
};
