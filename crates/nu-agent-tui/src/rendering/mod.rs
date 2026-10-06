pub use nu_agent_core::transcript::highlight;
pub mod layout;
pub mod modal;
pub mod selection;
pub mod theme;

#[cfg(test)]
#[path = "../../test/rendering/layout.rs"]
mod layout_test;

#[cfg(test)]
#[path = "../../test/rendering/modal.rs"]
mod modal_test;

#[cfg(test)]
#[path = "../../test/rendering/selection.rs"]
mod selection_test;

#[cfg(test)]
#[path = "../../test/rendering/theme.rs"]
mod theme_test;
