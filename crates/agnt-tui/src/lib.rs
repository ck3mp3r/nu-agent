pub mod interaction;
pub mod interactive;
pub mod markdown;
pub mod platform;
pub mod rendering;
pub mod runtime;
pub mod state;
pub mod widgets;

#[cfg(test)]
#[path = "../test/architecture/test.rs"]
mod architecture_test;

#[cfg(test)]
#[path = "../test/architecture/push_entry.rs"]
mod architecture_push_entry_test;

#[cfg(test)]
#[path = "../test/renderer.rs"]
mod renderer_test;

#[cfg(test)]
#[path = "../test/support.rs"]
pub(crate) mod test_support;

// Re-export primary public types
pub use interactive::TuiInteractiveUi;
pub use widgets::RatatuiBackend;

// Re-export runtime types needed by the plugin crate
pub use runtime::{
    AnsiTerminalBackend, HybridTerminalEvents, RuntimeRunError, TuiRuntimeRenderer,
    run_with_terminal_restore,
};
