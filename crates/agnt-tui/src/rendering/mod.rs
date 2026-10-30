pub use agnt_core::transcript::highlight;
pub mod chrome;
pub mod layout;
pub mod modal;
pub mod selection;
pub mod theme;

#[cfg(test)]
#[path = "../../test/rendering/chrome.rs"]
mod chrome_test;

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
#[path = "../../test/rendering/theme/support.rs"]
mod theme_support_test;

#[cfg(test)]
#[path = "../../test/rendering/theme/palette.rs"]
mod theme_palette_test;

#[cfg(test)]
#[path = "../../test/rendering/theme/theme_name.rs"]
mod theme_name_test;

#[cfg(test)]
#[path = "../../test/rendering/theme/diff.rs"]
mod theme_diff_test;

#[cfg(test)]
#[path = "../../test/rendering/theme/markdown.rs"]
mod theme_markdown_test;
