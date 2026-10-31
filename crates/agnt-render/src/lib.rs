//! Backend-neutral render layer: the pure planner, the backend seam, and the
//! plain-text baseline.
//!
//! No terminal library lives here. A backend (TUI, ANSI, plain text)
//! implements [`RenderBackend`]; [`LayoutPlanner`] turns entries plus a width
//! into a [`FramePlan`] the backend paints.

// region:    --- Modules

pub mod backend;
pub mod catalog;
pub mod gallery;
pub mod plain;
pub mod planner;
pub mod theme;

pub use backend::{RenderBackend, RowRect};
pub use catalog::{
    CallLineRow, CatalogComponent, CodeSurface, DiffView, LaneKind, LaneLine, StatRow, SurfaceRow,
};
pub use gallery::{
    Gallery, GalleryComponentSample, GalleryComponentSnapshot, GallerySample, GallerySnapshot,
};
pub use plain::PlainBackend;
pub use planner::{EntryFamily, FramePlan, LayoutPlanner, PlannedEntry};
pub use theme::{Color, Markdown, Theme};

// endregion: --- Modules
